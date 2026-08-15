use std::{env, time::Instant};

use foyer_shell_audio::{AudioConfig, AudioEvent, NarrationRuntime};
use foyer_shell_protocol::{CueAction, NarrationAnchor, NarrationBeat};

fn main() {
    let root = env::current_dir().expect("current directory");
    let runtime = NarrationRuntime::spawn(AudioConfig::for_workspace(&root));
    runtime
        .requests
        .send_blocking(NarrationBeat {
            id: "audio-smoke-one".into(),
            text: "Two pieces of evidence converge on the cache key, which explains why the first window had to wait for plugin discovery.".into(),
            style: foyer_shell_protocol::NarrationStyle::Determined,
            style_degree: 1.15,
            focus: vec!["evidence:one".into(), "evidence:two".into()],
            anchors: vec![NarrationAnchor {
                phrase: "cache key".into(),
                at_char: None,
                cue: CueAction::Emphasize {
                    ids: vec!["decision:cache".into()],
                },
            }],
        })
        .expect("queue narration");
    runtime
        .requests
        .send_blocking(NarrationBeat {
            id: "audio-smoke-two".into(),
            text: "With a stable key, that scan can disappear from the startup path.".into(),
            style: foyer_shell_protocol::NarrationStyle::Relieved,
            style_degree: 1.1,
            focus: vec!["decision:cache".into()],
            anchors: Vec::new(),
        })
        .expect("queue second narration");
    drop(runtime.requests);

    let started = Instant::now();
    let mut last_position_bucket = u64::MAX;
    let mut finished = 0;
    let mut second_synthesized_while_first_played = false;
    while let Ok(event) = runtime.events.recv_blocking() {
        if matches!(
            &event,
            AudioEvent::Synthesizing { beat_id } if beat_id == "audio-smoke-two" && finished == 0
        ) {
            second_synthesized_while_first_played = true;
        }
        let should_print = match &event {
            AudioEvent::Position { position_ms, .. } => {
                let bucket = position_ms / 500;
                if bucket == last_position_bucket {
                    false
                } else {
                    last_position_bucket = bucket;
                    true
                }
            }
            _ => true,
        };
        if should_print {
            println!("{:>5} ms  {event:?}", started.elapsed().as_millis());
        }
        match event {
            AudioEvent::PlaybackFinished { .. } => {
                finished += 1;
                if finished == 2 {
                    break;
                }
            }
            AudioEvent::Failed { .. } => break,
            _ => {}
        }
    }
    assert!(
        second_synthesized_while_first_played,
        "the second beat was not synthesized ahead of playback"
    );
}
