//! Round trips every `.vrtx` in `$VRTX_SAMPLES`. Handy for checking files we
//! can't ship in the repo, skipped when the variable isn't set.

use std::fs;

use vortexstudio_mcp::vrtx;

#[test]
fn external_samples_roundtrip() {
    let Some(dir) = std::env::var_os("VRTX_SAMPLES") else {
        eprintln!("VRTX_SAMPLES not set, skipping");
        return;
    };
    let mut seen = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "vrtx") {
            continue;
        }
        let file = fs::read(&path).unwrap();
        let (version, payload) = vrtx::unwrap_container(&file).unwrap();
        let project = vrtx::decode_payload(version, &payload)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        // older versions get upgraded on write, so only current ones must match exactly
        if version == vrtx::CURRENT_VERSION {
            assert_eq!(
                vrtx::encode_payload(&project),
                payload,
                "{}",
                path.display()
            );
        }
        let back = vrtx::decode(&vrtx::encode(&project).unwrap()).unwrap();
        assert_eq!(back.instances, project.instances, "{}", path.display());
        seen += 1;
    }
    assert!(seen > 0, "no .vrtx files found");
    eprintln!("checked {seen} samples");
}
