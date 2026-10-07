use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

const MPV_EXE: &str = r"C:\Program Files\MPV Player\mpv.exe";
const PIPE_NAME: &str = r"\\.\pipe\simple_mkv_player_mpv";

pub struct MpvPlayer {
    child: Option<Child>,
}

impl MpvPlayer {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        self.stop();

        if !Path::new(MPV_EXE).exists() {
            return Err(format!("mpv.exe not found at {MPV_EXE}"));
        }

        let child = Command::new(MPV_EXE)
            .arg("--no-config")
            .arg("--force-window=yes")
            .arg("--keep-open=yes")
            .arg("--pause=yes")
            .arg("--idle=no")
            .arg("--osc=yes")
            .arg("--input-default-bindings=yes")
            .arg(format!("--input-ipc-server={PIPE_NAME}"))
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start mpv: {e}"))?;

        self.child = Some(child);

        // mpv needs a moment to create the Windows named pipe.
        for _ in 0..50 {
            if self.can_connect() {
                return Ok(());
            }

            thread::sleep(Duration::from_millis(50));
        }

        Err("mpv started, but its IPC pipe did not become available.".into())
    }

    fn can_connect(&self) -> bool {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_NAME)
            .is_ok()
    }

    fn request(&self, command: Value) -> Result<Value, String> {
        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_NAME)
            .map_err(|e| format!("Could not connect to mpv IPC: {e}"))?;

        let request = json!({
            "command": command,
            "request_id": 1
        });

        let mut encoded = serde_json::to_vec(&request)
            .map_err(|e| format!("Could not encode mpv command: {e}"))?;

        encoded.push(b'\n');

        pipe.write_all(&encoded)
            .map_err(|e| format!("Could not send command to mpv: {e}"))?;

        pipe.flush()
            .map_err(|e| format!("Could not flush mpv command: {e}"))?;

        let mut reader = BufReader::new(pipe);
        let mut line = String::new();

        loop {
            line.clear();

            let bytes = reader
                .read_line(&mut line)
                .map_err(|e| format!("Could not read mpv response: {e}"))?;

            if bytes == 0 {
                return Err("mpv IPC closed before returning a response.".into());
            }

            let response: Value = serde_json::from_str(line.trim())
                .map_err(|e| format!("Invalid JSON from mpv: {e}"))?;

            // Ignore asynchronous events.
            if response.get("request_id").and_then(Value::as_i64) == Some(1) {
                if response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("success")
                    != "success"
                {
                    return Err(format!(
                        "mpv error: {}",
                        response["error"]
                    ));
                }

                return Ok(response);
            }
        }
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        self.request(json!(["set_property", "pause", paused]))?;
        Ok(())
    }

    pub fn paused(&self) -> Result<bool, String> {
        let response =
            self.request(json!(["get_property", "pause"]))?;

        response["data"]
            .as_bool()
            .ok_or_else(|| "mpv returned an invalid pause value.".into())
    }

    pub fn position(&self) -> Result<f64, String> {
        let response =
            self.request(json!(["get_property", "time-pos"]))?;

        Ok(response["data"].as_f64().unwrap_or(0.0))
    }

    pub fn duration(&self) -> Result<f64, String> {
        let response =
            self.request(json!(["get_property", "duration"]))?;

        Ok(response["data"].as_f64().unwrap_or(0.0))
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), String> {
        self.request(json!([
            "seek",
            seconds,
            "absolute+exact"
        ]))?;

        Ok(())
    }

    pub fn stop(&mut self) {
        if self.child.is_some() {
            let _ = self.request(json!(["quit"]));
        }

        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            },
            None => false,
        }
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
