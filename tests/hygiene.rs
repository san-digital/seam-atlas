use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn repository_hygiene_and_style_gate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // 1. Cargo.toml must declare zero runtime dependencies.
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("read Cargo.toml");
    let deps_section = cargo_toml
        .split("[dependencies]")
        .nth(1)
        .expect("missing [dependencies] section");
    let dep_body = deps_section.split('[').next().unwrap_or("");
    for line in dep_body.lines() {
        let trimmed = line.trim();
        assert!(
            trimmed.is_empty() || trimmed.starts_with('#'),
            "unexpected runtime dependency line in Cargo.toml: {trimmed}"
        );
    }

    // 2. Crate root must enforce #![forbid(unsafe_code)].
    let lib_rs = fs::read_to_string(root.join("src/lib.rs")).expect("read src/lib.rs");
    assert!(lib_rs.contains("#![forbid(unsafe_code)]"));

    // 3. Scan all repository source, policy, proof, and documentation files.
    let mut files = Vec::new();
    collect_files(&root, &mut files);

    let banned_words: &[&str] = &[
        concat!("grace", "fully"),
        concat!("seam", "lessly"),
        concat!("seam", "less"),
        concat!("del", "ve"),
        concat!("pivo", "tal"),
        concat!("cruc", "ial"),
        concat!("nota", "bly"),
        concat!("further", "more"),
        concat!("more", "over"),
        concat!("tape", "stry"),
        concat!("game-", "changer"),
        concat!("co-authored", "-by"),
        concat!("cod", "ex"),
        concat!("chat", "gpt"),
        concat!("clau", "de"),
        concat!("gem", "ini"),
        concat!("open", "ai"),
        concat!("anthro", "pic"),
        concat!("antigra", "vity"),
        concat!("deep", "mind"),
    ];

    let banned_phrases: &[&str] = &[concat!("rather ", "than")];

    for file in files {
        let rel = file.strip_prefix(&root).unwrap_or(&file);
        if rel.starts_with("target") || rel.starts_with(".git") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        let is_rs = file.extension().and_then(|e| e.to_str()) == Some("rs");
        let is_self = rel == Path::new("tests/hygiene.rs");

        if is_rs && !is_self {
            assert!(
                !content.contains("unsafe ") && !content.contains("unsafe{"),
                "unsafe block found in {}",
                rel.display()
            );
        }

        for (idx, line) in content.lines().enumerate() {
            let ln = idx + 1;
            for ch in [
                '\u{2014}', '\u{2013}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{00a0}',
                '\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}',
            ] {
                assert!(
                    !line.contains(ch),
                    "forbidden Unicode character U+{:04X} in {}:{}",
                    ch as u32,
                    rel.display(),
                    ln
                );
            }

            if is_self {
                continue;
            }

            let lower = line.to_ascii_lowercase();
            for phrase in banned_phrases {
                assert!(
                    !lower.contains(phrase),
                    "banned phrase '{phrase}' in {}:{}",
                    rel.display(),
                    ln
                );
            }
            let words: Vec<&str> = lower
                .split(|c: char| !c.is_ascii_alphabetic() && c != '-')
                .filter(|s| !s.is_empty())
                .collect();
            for bw in banned_words {
                assert!(
                    !words.contains(bw),
                    "banned word '{bw}' in {}:{}",
                    rel.display(),
                    ln
                );
            }
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        let name = ent.file_name();
        if name == "target" || name == ".git" {
            continue;
        }
        if p.is_dir() {
            collect_files(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if matches!(
                ext,
                "rs" | "toml" | "md" | "lean" | "sh" | "py" | "yml" | "yaml" | "policy" | "txt"
            ) {
                out.push(p);
            }
        }
    }
}
