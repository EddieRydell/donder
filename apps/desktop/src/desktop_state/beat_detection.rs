use super::DesktopState;
use donder_sequence_api::{GuiDocumentRequest, SequenceBeatDetection};

impl DesktopState {
    /// Detect beats and downbeats in the open sequence's audio. This reads the
    /// audio file only; the caller creates mark collections through a GUI edit.
    pub(crate) fn detect_sequence_beats(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<SequenceBeatDetection, String> {
        let audio = self
            .resolve_sequence_audio(request)
            .ok_or("Choose the sequence's audio before detecting beats.")?;
        if !audio.exists {
            return Err(format!("Audio file not found: {}", audio.resolved_path));
        }
        let times = donder_audio_analysis::detect_beats(audio.resolved_path.as_ref())
            .map_err(|error| error.to_string())?;
        let seconds = |times: Vec<f64>| times.into_iter().map(|time| time as f32).collect();
        Ok(SequenceBeatDetection {
            beats_seconds: seconds(times.beats),
            downbeats_seconds: seconds(times.downbeats),
        })
    }
}
