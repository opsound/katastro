#!/bin/bash
# Producer only. Build both helper slices on an Apple Silicon Mac with Xcode.
set -euo pipefail
[[ "$(uname -sm)" = "Darwin arm64" ]]
recipe="$(cd "$(dirname "$0")" && pwd)"
work="${1:?supply an empty build directory}"
mkdir -p "$work"
cd "$work"
work="$PWD"
export MACOSX_DEPLOYMENT_TARGET=13.0
jobs="${BUILD_JOBS:-6}"
fetch() {
    local name="$1" url="$2" hash="$3"
    [[ -f "$name.tar.gz" ]] || curl --fail --location --retry 3 "$url" -o "$name.tar.gz"
    echo "$hash  $name.tar.gz" | shasum -a 256 --check --status
    mkdir -p "$name-src"
    tar -xzf "$name.tar.gz" -C "$name-src" --strip-components 1
}
fetch katago https://codeload.github.com/lightvector/KataGo/tar.gz/fd0723fdbc0e9d82cf269c9630af8c27c57c07c4 5d663965f8c8886c1022b89565b9f1841045397bf7eb79c5cd57ea5881556a8a
fetch protobuf https://codeload.github.com/protocolbuffers/protobuf/tar.gz/refs/tags/v3.20.3 9c0fd39c7a08dff543c643f0f4baf081988129a411b977a07c46221793605638
fetch eigen https://gitlab.com/libeigen/eigen/-/archive/3.4.0/eigen-3.4.0.tar.gz 8586084f71f9bde545ee7fa6d00288b264a2b7ac3607b974e54d13e7162c1c72
common=(-G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0)
cmake -S protobuf-src/cmake -B proto "${common[@]}" -DCMAKE_INSTALL_PREFIX="$work/static" \
    -Dprotobuf_BUILD_TESTS=OFF -Dprotobuf_WITH_ZLIB=OFF -Dprotobuf_BUILD_SHARED_LIBS=OFF
cmake --build proto --target install -j "$jobs"
# The converter uses the protobuf API. This pinned static version predates
# protobuf's Abseil dependency, so the helper only needs system libraries.
python3 - <<'PY'
from pathlib import Path
p = Path("katago-src/cpp/external/katagocoreml/CMakeLists.txt")
s = p.read_text()
a, b = s.index("find_package(Protobuf"), s.index("# Proto Files")
p.write_text(s[:a] + 'set(KATAGOCOREML_DEP_LDFLAGS "${Protobuf_LIBRARIES}" PARENT_SCOPE)\n' + s[b:])
p = Path("katago-src/cpp/core/global.cpp")
p.write_text(p.read_text().replace("quick_exit(EXIT_FAILURE)", "_Exit(EXIT_FAILURE)"))
PY
for arch in arm64 x86_64; do
    if [[ "$arch" = arm64 ]]; then
        backend=(-DUSE_BACKEND=METAL "-DCMAKE_Swift_FLAGS=-target arm64-apple-macosx13.0" -DProtobuf_PROTOC_EXECUTABLE="$work/static/bin/protoc"
            -DProtobuf_LIBRARIES="$work/static/lib/libprotobuf.a" -DProtobuf_INCLUDE_DIRS="$work/static/include")
    else
        backend=(-DUSE_BACKEND=EIGEN -DEIGEN3_INCLUDE_DIRS="$work/eigen-src" -DUSE_AVX2=OFF)
    fi
    cmake -S katago-src/cpp -B "$arch" "${common[@]}" "${backend[@]}" \
        -DCMAKE_OSX_ARCHITECTURES="$arch" -DNO_GIT_REVISION=1 \
        -DLIBZIP_LIBRARY=FALSE -DLIBZIP_INCLUDE_DIR_ZIP=FALSE -DLIBZIP_INCLUDE_DIR_ZIPCONF=FALSE
    cmake --build "$arch" --target katago -j "$jobs"
done
mkdir -p distribution/notices
lipo -create arm64/katago x86_64/katago -output distribution/katago
codesign --force -s - distribution/katago
for arch in arm64 x86_64; do
    otool -arch "$arch" -L distribution/katago | awk 'NR>1 && $1 !~ "^/(usr/lib/|System/Library/)" {print; bad=1} END {exit bad}'
    xcrun vtool -arch "$arch" -show-build distribution/katago | awk '/minos/ {found=1; if ($2 != "13.0") exit 1} END {if (!found) exit 1}'
done
cp katago-src/LICENSE distribution/notices/KataGo.txt
cp protobuf-src/LICENSE distribution/notices/protobuf.txt
cp eigen-src/COPYING* distribution/notices/
(cd katago-src && find cpp/external -type f \( -iname 'license*' -o -iname 'copying*' -o -name NOTICE \) -exec sh -c 'for f; do mkdir -p "../distribution/notices/$(dirname "$f")"; cp "$f" "../distribution/notices/$f"; done' sh {} +)
sed -n '3,/^ \*\//p' katago-src/cpp/core/sha2.cpp > distribution/notices/sha2.txt
head -n 30 katago-src/cpp/external/nlohmann_json/json.hpp > distribution/notices/json.txt
mkdir -p distribution/Helpers distribution/Resources/Models
mv distribution/katago distribution/Helpers/
mv distribution/notices distribution/Resources/Notices
cp "$recipe/models.LICENSE" distribution/Resources/Notices/
fetch_model() {
    local name="$1" url="$2" hash="$3"
    local path="distribution/Resources/Models/$name"
    curl --fail --location --retry 3 "$url" -o "$path"
    echo "$hash  $path" | shasum -a 256 --check --status
}
fetch_model review.bin.gz https://media.katagotraining.org/uploaded/networks/models/kata1/kata1-b18c384nbt-s9996604416-d4316597426.bin.gz 9d7a6afed8ff5b74894727e156f04f0cd36060a24824892008fbb6e0cba51f1d
fetch_model b18c384nbt-humanv0.bin.gz https://github.com/lightvector/KataGo/releases/download/v1.15.0/b18c384nbt-humanv0.bin.gz 637746e44f0efe00ad1245a50aa9bbf0716efe364c43965ead97bd6835d84ab5
COPYFILE_DISABLE=1 tar -czf katago-macos-universal.tar.gz -C distribution Helpers Resources
