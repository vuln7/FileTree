//! Plays audio right in the app. The sound device is opened only while something is playing,
//! so an app that isn't making sound doesn't hold on to one.

use rodio::source::Source;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct AudioPlayer {
    // Dropping the sink closes the device, so it has to live as long as the player.
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
    pub path: PathBuf,
    pub duration: f32,
}

impl AudioPlayer {
    pub fn open(path: &Path) -> Result<AudioPlayer, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let source = rodio::Decoder::try_from(file).map_err(|_| "This audio format can't be played here.".to_string())?;
        let duration = source.total_duration().map_or(0.0, |d| d.as_secs_f32());
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().map_err(|_| "No sound device is available.".to_string())?;
        sink.log_on_drop(false);
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(source);
        Ok(AudioPlayer { _sink: sink, player, path: path.to_path_buf(), duration })
    }

    pub fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn is_playing(&self) -> bool {
        !self.player.is_paused() && !self.player.empty()
    }

    pub fn finished(&self) -> bool {
        self.player.empty()
    }

    pub fn position(&self) -> f32 {
        self.player.get_pos().as_secs_f32()
    }

    pub fn seek(&self, seconds: f32) {
        let _ = self.player.try_seek(Duration::from_secs_f32(seconds.max(0.0)));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_plain_wav_decodes() {
        let path = std::env::temp_dir().join(format!("filetree-audio-{}.wav", std::process::id()));
        let samples = 4000u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + samples as usize * 2, 0);
        std::fs::write(&path, &wav).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let decoded = rodio::Decoder::try_from(file);
        let _ = std::fs::remove_file(&path);
        assert!(decoded.is_ok(), "{:?}", decoded.err());
    }
}
