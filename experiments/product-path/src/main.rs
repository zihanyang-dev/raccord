use std::{env, fmt::Write, fs, path::Path};

use raccord_product_path_experiment::{Clip, SemanticEdit, Timeline, Transaction, plan_edit};

fn fixture() -> Timeline {
    Timeline {
        revision: 0,
        clips: vec![
            Clip {
                id: "a".into(),
                source: "asset://a".into(),
                duration_frames: 72,
                audio_gain_db_milli: 0,
            },
            Clip {
                id: "b".into(),
                source: "asset://b".into(),
                duration_frames: 72,
                audio_gain_db_milli: 0,
            },
            Clip {
                id: "c".into(),
                source: "asset://c".into(),
                duration_frames: 96,
                audio_gain_db_milli: 0,
            },
        ],
        markers: Vec::new(),
        subtitles: Vec::new(),
        transitions: Vec::new(),
    }
}

fn ripple_delete_preview() -> raccord_product_path_experiment::Preview {
    let transaction = Transaction {
        base_revision: 0,
        edits: vec![SemanticEdit::RippleDelete {
            clip_id: "b".into(),
        }],
    };

    plan_edit(&fixture(), &transaction).expect("experiment is valid")
}

fn write_concat_file(output: &Path, media_dir: &Path) -> std::io::Result<()> {
    let preview = ripple_delete_preview();
    let mut contents = String::new();

    for clip in &preview.timeline.clips {
        let path = media_dir.join(format!("{}.mp4", clip.id));
        writeln!(&mut contents, "file '{}'", path.display()).expect("writing a String cannot fail");
    }

    fs::write(output, contents)
}

fn main() {
    let mut arguments = env::args().skip(1);

    if arguments.next().as_deref() == Some("write-concat") {
        let output = arguments
            .next()
            .expect("write-concat requires an output path");
        let media_dir = arguments
            .next()
            .expect("write-concat requires a media directory");
        write_concat_file(Path::new(&output), Path::new(&media_dir))
            .expect("writing the concat manifest should succeed");
        return;
    }

    for placement in ripple_delete_preview().timeline.placements() {
        println!(
            "{}: {}..{}",
            placement.id,
            placement.start_frame,
            placement.start_frame + placement.duration_frames
        );
    }
}
