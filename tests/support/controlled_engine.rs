//! Drive the fake KataGo process from an integration test over a loopback socket.
use katastro::engine::EngineConfig;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    net::{TcpListener, TcpStream},
    path::Path,
    time::Duration,
};

pub fn config(root: &Path, listener: &TcpListener) -> EngineConfig {
    let model = root.join("model");
    std::fs::write(
        &model,
        format!("controlled:{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    EngineConfig {
        analysis: Default::default(),
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    }
}
pub fn connection(listener: &TcpListener) -> BufReader<TcpStream> {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    BufReader::new(stream)
}
pub fn query(control: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    control
        .read_line(&mut line)
        .expect("expected a request from the controlled engine");
    serde_json::from_str(&line).unwrap()
}
