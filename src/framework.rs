use crate::artifact::{BuildMode, FrameworkDetection, FrameworkKind};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(Clone, Debug, Serialize)]
pub struct JarAnalysis {
    pub main_class: Option<String>,
    pub frameworks: Vec<FrameworkDetection>,
    pub native_image_metadata_present: bool,
    pub entry_count: usize,
}

pub fn analyze_jar(path: &Path) -> Result<JarAnalysis> {
    let file =
        File::open(path).with_context(|| format!("failed to open JAR {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read JAR/ZIP archive {}", path.display()))?;

    let mut entries = BTreeSet::new();
    let mut native_image_metadata_present = false;
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name().replace('\\', "/");
        if name.starts_with("META-INF/native-image/") {
            native_image_metadata_present = true;
        }
        entries.insert(name);
    }

    let main_class = read_main_class(&mut archive).ok().flatten();
    let frameworks = detect_frameworks(&entries, native_image_metadata_present);

    Ok(JarAnalysis {
        main_class,
        frameworks,
        native_image_metadata_present,
        entry_count: entries.len(),
    })
}

pub fn ensure_supported_for_mode(
    mode: BuildMode,
    analysis: &JarAnalysis,
    allow_unsupported_framework: bool,
) -> Result<()> {
    if allow_unsupported_framework || mode == BuildMode::LegacySnapshot {
        return Ok(());
    }

    let unsupported: Vec<_> = analysis
        .frameworks
        .iter()
        .filter(|framework| !framework.supported_in_native)
        .collect();
    if unsupported.is_empty() {
        return Ok(());
    }

    let descriptions = unsupported
        .iter()
        .map(|framework| framework.kind.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "JAR uses framework shape not safely supported in native mode yet: {descriptions}. Use `--mode legacy-snapshot` or pass `--allow-unsupported-framework` to force native-image."
    );
}

fn read_main_class(archive: &mut ZipArchive<File>) -> Result<Option<String>> {
    let Ok(mut manifest) = archive.by_name("META-INF/MANIFEST.MF") else {
        return Ok(None);
    };

    let mut content = String::new();
    manifest.read_to_string(&mut content)?;
    let mut unfolded = String::new();
    for line in content.lines() {
        if let Some(continuation) = line.strip_prefix(' ') {
            unfolded.push_str(continuation);
        } else {
            unfolded.push('\n');
            unfolded.push_str(line);
        }
    }

    for line in unfolded.lines() {
        if let Some(value) = line.strip_prefix("Main-Class:") {
            return Ok(Some(value.trim().to_string()));
        }
        if let Some(value) = line.strip_prefix("Start-Class:") {
            return Ok(Some(value.trim().to_string()));
        }
    }

    Ok(None)
}

fn detect_frameworks(
    entries: &BTreeSet<String>,
    native_image_metadata_present: bool,
) -> Vec<FrameworkDetection> {
    let mut detections = Vec::new();

    push_if_detected(
        &mut detections,
        entries,
        FrameworkKind::Micronaut,
        &["io/micronaut/", "META-INF/services/io.micronaut"],
        true,
        None,
    );
    push_if_detected(
        &mut detections,
        entries,
        FrameworkKind::Quarkus,
        &["io/quarkus/", "META-INF/quarkus"],
        true,
        None,
    );

    let spring_evidence = evidence_for(entries, &["BOOT-INF/", "org/springframework/boot/"]);
    if !spring_evidence.is_empty() {
        detections.push(FrameworkDetection {
            kind: FrameworkKind::SpringBoot,
            confidence: "high".to_string(),
            evidence: spring_evidence,
            supported_in_native: native_image_metadata_present,
            recommendation: if native_image_metadata_present {
                None
            } else {
                Some(
                    "Spring Boot native mode requires Spring AOT/native-image metadata; otherwise use legacy-snapshot."
                        .to_string(),
                )
            },
        });
    }

    let servlet_evidence = evidence_for(
        entries,
        &["WEB-INF/web.xml", "jakarta/servlet/", "javax/servlet/"],
    );
    if !servlet_evidence.is_empty() {
        detections.push(FrameworkDetection {
            kind: FrameworkKind::ServletWar,
            confidence: "high".to_string(),
            evidence: servlet_evidence,
            supported_in_native: false,
            recommendation: Some(
                "Servlet/WAR style applications should start with legacy-snapshot mode."
                    .to_string(),
            ),
        });
    }

    if detections.is_empty() {
        detections.push(FrameworkDetection {
            kind: FrameworkKind::PlainJava,
            confidence: "medium".to_string(),
            evidence: Vec::new(),
            supported_in_native: true,
            recommendation: None,
        });
    }

    detections
}

fn push_if_detected(
    detections: &mut Vec<FrameworkDetection>,
    entries: &BTreeSet<String>,
    kind: FrameworkKind,
    prefixes: &[&str],
    supported_in_native: bool,
    recommendation: Option<String>,
) {
    let evidence = evidence_for(entries, prefixes);
    if evidence.is_empty() {
        return;
    }

    detections.push(FrameworkDetection {
        kind,
        confidence: "high".to_string(),
        evidence,
        supported_in_native,
        recommendation,
    });
}

fn evidence_for(entries: &BTreeSet<String>, prefixes: &[&str]) -> Vec<String> {
    let mut evidence = Vec::new();
    for prefix in prefixes {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.as_str() == *prefix || entry.starts_with(prefix))
        {
            evidence.push(entry.clone());
        }
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_plain_java_jar() {
        let temp = tempfile::tempdir().unwrap();
        let jar = temp.path().join("plain.jar");
        write_test_jar(&jar, &["com/example/App.class"], Some("com.example.App"));

        let analysis = analyze_jar(&jar).unwrap();
        assert_eq!(analysis.main_class.as_deref(), Some("com.example.App"));
        assert_eq!(analysis.frameworks[0].kind, FrameworkKind::PlainJava);
        assert!(analysis.frameworks[0].supported_in_native);
    }

    #[test]
    fn rejects_spring_without_native_metadata_for_native_mode() {
        let temp = tempfile::tempdir().unwrap();
        let jar = temp.path().join("spring.jar");
        write_test_jar(&jar, &["BOOT-INF/classes/com/example/App.class"], None);

        let analysis = analyze_jar(&jar).unwrap();
        assert_eq!(analysis.frameworks[0].kind, FrameworkKind::SpringBoot);
        assert!(ensure_supported_for_mode(BuildMode::Native, &analysis, false).is_err());
        assert!(ensure_supported_for_mode(BuildMode::LegacySnapshot, &analysis, false).is_ok());
    }

    #[test]
    fn accepts_spring_with_native_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let jar = temp.path().join("spring-native.jar");
        write_test_jar(
            &jar,
            &[
                "BOOT-INF/classes/com/example/App.class",
                "META-INF/native-image/com.example/app/reflect-config.json",
            ],
            None,
        );

        let analysis = analyze_jar(&jar).unwrap();
        assert!(analysis.native_image_metadata_present);
        assert!(ensure_supported_for_mode(BuildMode::Native, &analysis, false).is_ok());
    }

    fn write_test_jar(path: &Path, entries: &[&str], main_class: Option<&str>) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        let manifest = match main_class {
            Some(main_class) => format!("Manifest-Version: 1.0\nMain-Class: {main_class}\n"),
            None => "Manifest-Version: 1.0\n".to_string(),
        };
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        for entry in entries {
            zip.start_file(entry, options).unwrap();
            zip.write_all(b"test").unwrap();
        }
        zip.finish().unwrap();
    }
}
