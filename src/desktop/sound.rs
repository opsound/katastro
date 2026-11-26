use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};

const TABLETOP_STRIKES: [&[u8]; 3] = [
    include_bytes!("../../assets/stone-1.wav"),
    include_bytes!("../../assets/stone-2.wav"),
    include_bytes!("../../assets/stone-3.wav"),
];

type Playback = Box<dyn FnMut(&'static [u8])>;

pub(super) struct StoneSound {
    output: Playback,
    order: [&'static [u8]; TABLETOP_STRIKES.len()],
    next: usize,
    rng: SmallRng,
}

impl StoneSound {
    pub(super) fn new(play: impl FnMut(&'static [u8]) + 'static) -> Self {
        Self::seeded(play, rand::random())
    }

    pub(super) fn seeded(play: impl FnMut(&'static [u8]) + 'static, seed: u64) -> Self {
        let mut rng = SmallRng::seed_from_u64(seed);
        let mut order = TABLETOP_STRIKES;
        order.shuffle(&mut rng);
        Self {
            output: Box::new(play),
            order,
            next: 0,
            rng,
        }
    }

    pub(super) fn play(&mut self, count: usize, muted: bool) {
        if !muted {
            for _ in 0..count {
                if self.next == self.order.len() {
                    // Choose an opener from all but the last strike, then shuffle the rest.
                    let last = self.order.len() - 1;
                    self.order[..last].shuffle(&mut self.rng);
                    self.order[1..].shuffle(&mut self.rng);
                    self.next = 0;
                }
                (self.output)(self.order[self.next]);
                self.next += 1;
            }
        }
    }
}

impl Default for StoneSound {
    #[cfg(target_os = "macos")]
    fn default() -> Self {
        let mut voices = Vec::new();
        Self::new(move |wav| {
            play_native(&mut voices, wav);
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn default() -> Self {
        Self::new(|_| {})
    }
}

#[cfg(target_os = "macos")]
struct Voice {
    wav: &'static [u8],
    sound: objc2::rc::Retained<objc2_app_kit::NSSound>,
}

#[cfg(target_os = "macos")]
fn play_native(voices: &mut Vec<Voice>, wav: &'static [u8]) -> bool {
    use objc2::AnyThread;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    if let Some(idle) = voices
        .iter()
        .find(|voice| std::ptr::eq(voice.wav, wav) && !voice.sound.isPlaying())
    {
        return idle.sound.play();
    }
    let data = NSData::with_bytes(wav);
    let Some(voice) = NSSound::initWithData(NSSound::alloc(), &data) else {
        return false;
    };
    let started = voice.play();
    voices.push(Voice { wav, sound: voice });
    started
}

#[cfg(all(test, target_os = "macos"))]
#[test]
#[ignore = "plays all three tabletop strikes through the default macOS audio output"]
fn native_stone_playback() {
    let mut voices = Vec::new();
    for wav in TABLETOP_STRIKES {
        assert!(
            play_native(&mut voices, wav),
            "AppKit could not start a tabletop strike"
        );
        let voice = voices
            .iter()
            .find(|v| std::ptr::eq(v.wav, wav))
            .expect("AppKit reused a different strike");
        // Audition each take separately and leave an idle voice for the next request.
        std::thread::sleep(std::time::Duration::from_secs_f64(voice.sound.duration()));
        voice.sound.stop();
    }
}
