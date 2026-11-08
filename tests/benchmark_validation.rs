use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn get_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn test_requirements_pinned() {
    let req_path = get_workspace_root().join("bench").join("requirements.txt");
    assert!(req_path.exists(), "bench/requirements.txt must exist");

    let content = fs::read_to_string(&req_path).expect("read requirements.txt");
    let mut has_pillow = false;
    let mut has_numpy = false;
    let mut has_skimage = false;
    let mut has_thumbor = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        assert!(
            line.contains("=="),
            "Requirement '{line}' must be pinned with =="
        );
        if line.starts_with("Pillow==") {
            has_pillow = true;
        }
        if line.starts_with("numpy==") {
            has_numpy = true;
        }
        if line.starts_with("scikit-image==") {
            has_skimage = true;
        }
        if line.starts_with("thumbor==") {
            has_thumbor = true;
        }
    }

    assert!(has_pillow, "requirements.txt must pin Pillow");
    assert!(has_numpy, "requirements.txt must pin numpy");
    assert!(has_skimage, "requirements.txt must pin scikit-image");
    assert!(has_thumbor, "requirements.txt must pin thumbor");
}

#[test]
fn test_compose_ports_bind_strictly_to_localhost() {
    let compose_path = get_workspace_root()
        .join("bench")
        .join("docker-compose.bench.yml");
    assert!(compose_path.exists(), "docker-compose.bench.yml must exist");

    let content = fs::read_to_string(&compose_path).expect("read compose file");
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- \"") && trimmed.contains(':') {
            let port_spec = trimmed.trim_matches(|c| c == '-' || c == '"' || c == ' ');
            assert!(
                port_spec.starts_with("127.0.0.1:"),
                "Compose port mapping '{port_spec}' must bind strictly to 127.0.0.1"
            );
        }
    }
}

#[test]
fn test_corpus_manifest_integrity() {
    let manifest_path = get_workspace_root()
        .join("bench")
        .join("corpus_manifest.csv");
    let corpus_dir = get_workspace_root().join("benchmarks").join("corpus");

    assert!(manifest_path.exists(), "corpus_manifest.csv must exist");
    assert!(corpus_dir.exists(), "benchmarks/corpus must exist");

    let content = fs::read_to_string(&manifest_path).expect("read manifest");
    let mut count = 0;

    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue; // Skip CSV header
        }
        // Split with care for quoted fields: the last field is sha256, second is filename
        let parts: Vec<&str> = line.split(',').collect();
        let filename = parts[1].trim();
        let expected_sha = parts.last().unwrap().trim();

        let img_path = corpus_dir.join(filename);
        assert!(img_path.exists(), "Corpus image missing: {filename}");

        let bytes = fs::read(&img_path).expect("read image");
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual_sha = hex::encode(hasher.finalize());

        assert_eq!(
            actual_sha, expected_sha,
            "SHA256 checksum mismatch for {filename}"
        );
        count += 1;
    }

    assert_eq!(count, 20, "Corpus must have exactly 20 verified images");
}

#[test]
fn test_benchmark_python_validation_suite() {
    let root = get_workspace_root();
    let script = root.join("bench").join("run_all.sh");
    assert!(script.exists(), "bench/run_all.sh must exist");

    let status = Command::new("bash")
        .arg(&script)
        .arg("--validate-only")
        .current_dir(&root)
        .status()
        .expect("execute run_all.sh --validate-only");

    assert!(
        status.success(),
        "run_all.sh --validate-only must exit with code 0"
    );
}

#[test]
fn test_fallback_candidate_records_and_failure_modes() {
    let root = get_workspace_root();
    let script = root.join("bench").join("test_candidate_validation.py");
    assert!(
        script.exists(),
        "bench/test_candidate_validation.py must exist"
    );

    let venv_python = root.join(".venv_bench").join("bin").join("python3");
    let python_bin = if venv_python.exists() {
        venv_python
    } else {
        PathBuf::from("python3")
    };

    let status = Command::new(&python_bin)
        .arg(&script)
        .current_dir(&root)
        .status()
        .expect("execute test_candidate_validation.py");

    assert!(
        status.success(),
        "test_candidate_validation.py must exit with code 0"
    );
}
