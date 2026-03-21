use crate::ArtifactBuildRequest;
use crate::ArtifactCommandOutput;
use crate::ArtifactRuntimeManager;
use crate::ArtifactRuntimeManagerConfig;
use crate::ArtifactRuntimePlatform;
use crate::ArtifactRuntimeReleaseLocator;
use crate::ArtifactsClient;
use crate::DEFAULT_CACHE_ROOT_RELATIVE;
use crate::ReleaseManifest;
use crate::load_cached_runtime;
use codex_package_manager::ArchiveFormat;
use codex_package_manager::PackageReleaseArchive;
use flate2::Compression;
use flate2::write::GzEncoder;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tar::Builder as TarBuilder;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

#[test]
fn release_locator_builds_manifest_url() {
    let locator = ArtifactRuntimeReleaseLocator::new(
        url::Url::parse("https://example.test/releases/").unwrap_or_else(|error| panic!("{error}")),
        "0.1.0",
    );
    let url = locator
        .manifest_url()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        url.as_str(),
        "https://example.test/releases/artifact-runtime-v0.1.0/artifact-runtime-v0.1.0-manifest.json"
    );
}

#[test]
fn default_release_locator_uses_openai_codex_github_releases() {
    let locator = ArtifactRuntimeReleaseLocator::default("0.1.0");
    let url = locator
        .manifest_url()
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        url.as_str(),
        "https://github.com/openai/codex/releases/download/artifact-runtime-v0.1.0/artifact-runtime-v0.1.0-manifest.json"
    );
}

#[test]
fn load_cached_runtime_reads_installed_runtime() {
    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_version = "2.5.6";
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let install_dir = codex_home
        .path()
        .join(DEFAULT_CACHE_ROOT_RELATIVE)
        .join(runtime_version)
        .join(platform.as_str());
    write_installed_runtime(&install_dir, runtime_version);

    let runtime = load_cached_runtime(
        &codex_home.path().join(DEFAULT_CACHE_ROOT_RELATIVE),
        runtime_version,
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(runtime.runtime_version(), runtime_version);
    assert_eq!(runtime.platform(), platform);
    assert!(
        runtime
            .build_js_path()
            .ends_with(Path::new("dist/artifact_tool.mjs"))
    );
}

#[test]
fn load_cached_runtime_requires_build_entrypoint() {
    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_version = "2.5.6";
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let install_dir = codex_home
        .path()
        .join(DEFAULT_CACHE_ROOT_RELATIVE)
        .join(runtime_version)
        .join(platform.as_str());
    write_installed_runtime(&install_dir, runtime_version);
    fs::remove_file(install_dir.join("dist/artifact_tool.mjs"))
        .unwrap_or_else(|error| panic!("{error}"));

    let error = load_cached_runtime(
        &codex_home.path().join(DEFAULT_CACHE_ROOT_RELATIVE),
        runtime_version,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "required runtime file is missing: {}",
            install_dir.join("dist/artifact_tool.mjs").display()
        )
    );
}

#[tokio::test]
async fn ensure_installed_downloads_and_extracts_zip_runtime() {
    let server = MockServer::start().await;
    let runtime_version = "2.5.6";
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let archive_name = format!(
        "artifact-runtime-v{runtime_version}-{}.zip",
        platform.as_str()
    );
    let archive_bytes = build_zip_archive(runtime_version);
    let archive_sha = format!("{:x}", Sha256::digest(&archive_bytes));
    let manifest = ReleaseManifest {
        schema_version: 1,
        runtime_version: runtime_version.to_string(),
        release_tag: format!("artifact-runtime-v{runtime_version}"),
        node_version: None,
        platforms: BTreeMap::from([(
            platform.as_str().to_string(),
            PackageReleaseArchive {
                archive: archive_name.clone(),
                sha256: archive_sha,
                format: ArchiveFormat::Zip,
                size_bytes: Some(archive_bytes.len() as u64),
            },
        )]),
    };
    Mock::given(method("GET"))
        .and(path(format!(
            "/artifact-runtime-v{runtime_version}/artifact-runtime-v{runtime_version}-manifest.json"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/artifact-runtime-v{runtime_version}/{archive_name}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive_bytes))
        .mount(&server)
        .await;

    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let locator = ArtifactRuntimeReleaseLocator::new(
        url::Url::parse(&format!("{}/", server.uri())).unwrap_or_else(|error| panic!("{error}")),
        runtime_version,
    );
    let manager = ArtifactRuntimeManager::new(ArtifactRuntimeManagerConfig::new(
        codex_home.path().to_path_buf(),
        locator,
    ));

    let runtime = manager
        .ensure_installed()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(runtime.runtime_version(), runtime_version);
    assert_eq!(runtime.platform(), platform);
    assert!(
        runtime
            .build_js_path()
            .ends_with(Path::new("dist/artifact_tool.mjs"))
    );
}

#[test]
fn load_cached_runtime_requires_package_export() {
    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_version = "2.5.6";
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let install_dir = codex_home
        .path()
        .join(DEFAULT_CACHE_ROOT_RELATIVE)
        .join(runtime_version)
        .join(platform.as_str());
    write_installed_runtime(&install_dir, runtime_version);
    fs::write(
        install_dir.join("package.json"),
        serde_json::json!({
            "name": "@oai/artifact-tool",
            "version": runtime_version,
            "type": "module",
        })
        .to_string(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    let error = load_cached_runtime(
        &codex_home.path().join(DEFAULT_CACHE_ROOT_RELATIVE),
        runtime_version,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "invalid package metadata at {}",
            install_dir.join("package.json").display()
        )
    );
}

#[tokio::test]
async fn ensure_installed_downloads_and_extracts_tar_gz_runtime() {
    let server = MockServer::start().await;
    let runtime_version = "2.5.6";
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let archive_name = format!(
        "artifact-runtime-v{runtime_version}-{}.tar.gz",
        platform.as_str()
    );
    let archive_bytes = build_tar_gz_archive(runtime_version);
    let archive_sha = format!("{:x}", Sha256::digest(&archive_bytes));
    let manifest = ReleaseManifest {
        schema_version: 1,
        runtime_version: runtime_version.to_string(),
        release_tag: format!("artifact-runtime-v{runtime_version}"),
        node_version: None,
        platforms: BTreeMap::from([(
            platform.as_str().to_string(),
            PackageReleaseArchive {
                archive: archive_name.clone(),
                sha256: archive_sha,
                format: ArchiveFormat::TarGz,
                size_bytes: Some(archive_bytes.len() as u64),
            },
        )]),
    };
    Mock::given(method("GET"))
        .and(path(format!(
            "/artifact-runtime-v{runtime_version}/artifact-runtime-v{runtime_version}-manifest.json"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/artifact-runtime-v{runtime_version}/{archive_name}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive_bytes))
        .mount(&server)
        .await;

    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let locator = ArtifactRuntimeReleaseLocator::new(
        url::Url::parse(&format!("{}/", server.uri())).unwrap_or_else(|error| panic!("{error}")),
        runtime_version,
    );
    let manager = ArtifactRuntimeManager::new(ArtifactRuntimeManagerConfig::new(
        codex_home.path().to_path_buf(),
        locator,
    ));

    let runtime = manager
        .ensure_installed()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(runtime.runtime_version(), runtime_version);
    assert_eq!(runtime.platform(), platform);
    assert!(
        runtime
            .build_js_path()
            .ends_with(Path::new("dist/artifact_tool.mjs"))
    );
}

#[test]
fn load_cached_runtime_uses_custom_cache_root() {
    let codex_home = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_version = "2.5.6";
    let custom_cache_root = codex_home.path().join("runtime-cache");
    let platform =
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}"));
    let install_dir = custom_cache_root
        .join(runtime_version)
        .join(platform.as_str());
    write_installed_runtime(&install_dir, runtime_version);

    let config = ArtifactRuntimeManagerConfig::with_default_release(
        codex_home.path().to_path_buf(),
        runtime_version,
    )
    .with_cache_root(custom_cache_root);

    let runtime = load_cached_runtime(&config.cache_root(), runtime_version)
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(runtime.runtime_version(), runtime_version);
    assert_eq!(runtime.platform(), platform);
}

#[tokio::test]
#[cfg(unix)]
async fn artifacts_client_execute_build_writes_wrapped_script_and_env() {
    let temp = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_root = temp.path().join("runtime");
    write_installed_runtime(&runtime_root, "2.5.6");
    let runtime = crate::InstalledArtifactRuntime::load(
        runtime_root,
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let client = ArtifactsClient::from_installed_runtime(runtime);

    let output = client
        .execute_build(ArtifactBuildRequest {
            source: concat!(
                "console.log(typeof artifacts);\n",
                "console.log(typeof codexArtifacts);\n",
                "console.log(artifactTool.ok);\n",
                "console.log(ok);\n",
                "console.error('stderr-ok');\n",
                "console.log('stdout-ok');\n"
            )
            .to_string(),
            cwd: temp.path().to_path_buf(),
            timeout: Some(Duration::from_secs(5)),
            env: BTreeMap::new(),
        })
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_success(&output);
    assert_eq!(output.stderr.trim(), "stderr-ok");
    assert_eq!(
        output.stdout.lines().collect::<Vec<_>>(),
        vec!["undefined", "undefined", "true", "true", "stdout-ok"]
    );
}

#[tokio::test]
#[cfg(unix)]
async fn artifacts_client_execute_build_installs_builtin_presentation_layouts() {
    let temp = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_root = temp.path().join("runtime");
    write_installed_runtime_with_source(
        &runtime_root,
        "2.5.6",
        r#"class FakePlaceholder {
  constructor(proto) {
    this.proto = JSON.parse(JSON.stringify(proto));
  }

  toProto() {
    return JSON.parse(JSON.stringify(this.proto));
  }
}

class FakePlaceholderCollection {
  constructor(initialProtos = []) {
    this.protos = initialProtos.map((proto) => JSON.parse(JSON.stringify(proto)));
  }

  getAll() {
    return this.protos.map((proto) => new FakePlaceholder(proto));
  }
}

export class Slide {
  constructor(defaultProtos = []) {
    this.useLayoutId = null;
    this.#setPlaceholders(defaultProtos);
  }

  #setPlaceholders(protos) {
    this.placeholders = new FakePlaceholderCollection(protos);
  }

  setLayout(layout) {
    this.useLayoutId = layout.id;
    const protos = layout.placeholders.getAll().map((placeholder) => placeholder.toProto());
    this.#setPlaceholders(protos);
    return this;
  }
}

class SlideCollection {
  constructor(defaultProtos) {
    this.defaultProtos = defaultProtos;
  }

  add() {
    return new Slide(this.defaultProtos);
  }
}

export class Presentation {
  constructor() {
    this.slides = new SlideCollection([
      { id: "1", name: "Title 1", placeholderType: "ctrTitle" },
      { id: "2", name: "Subtitle 2", placeholderType: "subTitle" },
    ]);
  }

  static async create() {
    return new Presentation();
  }

  template(name) {
    throw new Error(`Unknown presentation template: ${name}`);
  }
}
"#,
    );
    let runtime = crate::InstalledArtifactRuntime::load(
        runtime_root,
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let client = ArtifactsClient::from_installed_runtime(runtime);

    let output = client
        .execute_build(ArtifactBuildRequest {
            source: concat!(
                "const presentation = await Presentation.create();\n",
                "console.log(presentation.template().layouts.getAll().map((layout) => layout.name).join(','));\n",
                "console.log(presentation.template('blank').id);\n",
                "const slide = presentation.slides.add();\n",
                "slide.setLayout('blank');\n",
                "console.log(slide.useLayoutId);\n",
                "console.log(slide.placeholders.getAll().length);\n",
                "slide.setLayout('title slide');\n",
                "console.log(slide.useLayoutId);\n",
                "console.log(slide.placeholders.getAll().length);\n"
            )
            .to_string(),
            cwd: temp.path().to_path_buf(),
            timeout: Some(Duration::from_secs(5)),
            env: BTreeMap::new(),
        })
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_success(&output);
    assert_eq!(
        output.stdout.lines().collect::<Vec<_>>(),
        vec![
            "Blank,Title Slide",
            "codex-layout-blank",
            "codex-layout-blank",
            "0",
            "codex-layout-title-slide",
            "2",
        ]
    );
}

#[tokio::test]
#[cfg(unix)]
async fn artifacts_client_execute_build_exports_title_slide_pptx_smoke() {
    let temp = TempDir::new().unwrap_or_else(|error| panic!("{error}"));
    let runtime_root = temp.path().join("runtime");
    write_installed_runtime_with_source(
        &runtime_root,
        "2.5.6",
        r#"const fs = await import("node:fs/promises");

class FakePlaceholder {
  constructor(proto) {
    this.proto = JSON.parse(JSON.stringify(proto));
    this.text = proto.text ?? "";
  }

  toProto() {
    return {
      ...JSON.parse(JSON.stringify(this.proto)),
      text: this.text,
    };
  }
}

class FakePlaceholderCollection {
  constructor(initialProtos = []) {
    this.replaceAll(initialProtos);
  }

  getAll() {
    return this.placeholders;
  }

  replaceAll(protos) {
    this.placeholders = protos.map((proto) => new FakePlaceholder(proto));
  }
}

export class Slide {
  constructor(defaultProtos = []) {
    this.placeholders = new FakePlaceholderCollection(defaultProtos);
    this.useLayoutId = null;
  }

  setLayout(layout) {
    this.useLayoutId = layout.id;
    const protos = layout.placeholders
      .getAll()
      .map((placeholder) => placeholder.toProto());
    this.placeholders.replaceAll(protos);
    return this;
  }
}

class SlideCollection {
  constructor(defaultProtos) {
    this.defaultProtos = defaultProtos;
    this.slides = [];
  }

  add() {
    const slide = new Slide(this.defaultProtos);
    this.slides.push(slide);
    return slide;
  }

  getAll() {
    return this.slides;
  }
}

export class Presentation {
  constructor() {
    this.slides = new SlideCollection([
      { id: "1", name: "Title 1", placeholderType: "ctrTitle", text: "" },
      { id: "2", name: "Subtitle 2", placeholderType: "subTitle", text: "" },
    ]);
  }

  static async create() {
    return new Presentation();
  }

  template(name) {
    throw new Error(`Unknown presentation template: ${name}`);
  }
}

function escapeXml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let index = 0; index < 8; index += 1) {
      const mask = -(crc & 1);
      crc = (crc >>> 1) ^ (0xedb88320 & mask);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function writeU16(buffer, offset, value) {
  buffer.writeUInt16LE(value & 0xffff, offset);
}

function writeU32(buffer, offset, value) {
  buffer.writeUInt32LE(value >>> 0, offset);
}

function createStoredZip(entries) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  for (const entry of entries) {
    const nameBytes = Buffer.from(entry.name, "utf8");
    const dataBytes = Buffer.isBuffer(entry.data)
      ? entry.data
      : Buffer.from(entry.data, "utf8");
    const localHeader = Buffer.alloc(30 + nameBytes.length);
    writeU32(localHeader, 0, 0x04034b50);
    writeU16(localHeader, 4, 20);
    writeU16(localHeader, 6, 0);
    writeU16(localHeader, 8, 0);
    writeU16(localHeader, 10, 0);
    writeU16(localHeader, 12, 0);
    writeU32(localHeader, 14, crc32(dataBytes));
    writeU32(localHeader, 18, dataBytes.length);
    writeU32(localHeader, 22, dataBytes.length);
    writeU16(localHeader, 26, nameBytes.length);
    writeU16(localHeader, 28, 0);
    nameBytes.copy(localHeader, 30);
    localParts.push(localHeader, dataBytes);

    const centralHeader = Buffer.alloc(46 + nameBytes.length);
    writeU32(centralHeader, 0, 0x02014b50);
    writeU16(centralHeader, 4, 20);
    writeU16(centralHeader, 6, 20);
    writeU16(centralHeader, 8, 0);
    writeU16(centralHeader, 10, 0);
    writeU16(centralHeader, 12, 0);
    writeU16(centralHeader, 14, 0);
    writeU32(centralHeader, 16, crc32(dataBytes));
    writeU32(centralHeader, 20, dataBytes.length);
    writeU32(centralHeader, 24, dataBytes.length);
    writeU16(centralHeader, 28, nameBytes.length);
    writeU16(centralHeader, 30, 0);
    writeU16(centralHeader, 32, 0);
    writeU16(centralHeader, 34, 0);
    writeU16(centralHeader, 36, 0);
    writeU32(centralHeader, 38, 0);
    writeU32(centralHeader, 42, offset);
    nameBytes.copy(centralHeader, 46);
    centralParts.push(centralHeader);

    offset += localHeader.length + dataBytes.length;
  }

  const centralDirectory = Buffer.concat(centralParts);
  const endOfCentralDirectory = Buffer.alloc(22);
  writeU32(endOfCentralDirectory, 0, 0x06054b50);
  writeU16(endOfCentralDirectory, 4, 0);
  writeU16(endOfCentralDirectory, 6, 0);
  writeU16(endOfCentralDirectory, 8, entries.length);
  writeU16(endOfCentralDirectory, 10, entries.length);
  writeU32(endOfCentralDirectory, 12, centralDirectory.length);
  writeU32(endOfCentralDirectory, 16, offset);
  writeU16(endOfCentralDirectory, 20, 0);

  return Buffer.concat([...localParts, centralDirectory, endOfCentralDirectory]);
}

export class FileBlob {
  constructor(bytes) {
    this.bytes = bytes;
  }

  async save(path) {
    await fs.writeFile(path, this.bytes);
  }
}

export class PresentationFile {
  static async exportPptx(presentation) {
    const slide = presentation.slides.getAll()[0];
    const placeholders = slide.placeholders.getAll();
    const title = placeholders[0]?.text ?? "";
    const subtitle = placeholders[1]?.text ?? "";
    const slideXml = `<?xml version="1.0" encoding="utf-8"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:r><a:t>${escapeXml(title)}</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:txBody><a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:r><a:t>${escapeXml(subtitle)}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>`;
    const bytes = createStoredZip([
      {
        name: "[Content_Types].xml",
        data: `<?xml version="1.0" encoding="utf-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>`,
      },
      {
        name: "_rels/.rels",
        data: `<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>`,
      },
      {
        name: "ppt/presentation.xml",
        data: `<?xml version="1.0" encoding="utf-8"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>`,
      },
      {
        name: "ppt/_rels/presentation.xml.rels",
        data: `<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>`,
      },
      {
        name: "ppt/slides/slide1.xml",
        data: slideXml,
      },
    ]);
    return new FileBlob(bytes);
  }
}
"#,
    );
    let runtime = crate::InstalledArtifactRuntime::load(
        runtime_root,
        ArtifactRuntimePlatform::detect_current().unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let client = ArtifactsClient::from_installed_runtime(runtime);
    let output_path = temp.path().join("smoke.pptx");
    let expected_output_path = output_path.display().to_string();

    let output = client
        .execute_build(ArtifactBuildRequest {
            source: format!(
                concat!(
                    "const presentation = await Presentation.create();\n",
                    "const slide = presentation.slides.add();\n",
                    "slide.setLayout('title slide');\n",
                    "const placeholders = slide.placeholders.getAll();\n",
                    "placeholders[0].text = 'Artifacts E2E Smoke';\n",
                    "placeholders[1].text = 'Patched codex binary executed artifacts successfully.';\n",
                    "const file = await PresentationFile.exportPptx(presentation);\n",
                    "await file.save({output_path:?});\n",
                    "console.log({output_path:?});\n"
                ),
                output_path = expected_output_path,
            ),
            cwd: temp.path().to_path_buf(),
            timeout: Some(Duration::from_secs(5)),
            env: BTreeMap::new(),
        })
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_success(&output);
    assert_eq!(output.stdout.trim(), expected_output_path);

    let archive = fs::File::open(&output_path).unwrap_or_else(|error| panic!("{error}"));
    let mut archive = ZipArchive::new(archive).unwrap_or_else(|error| panic!("{error}"));
    let mut slide_xml = String::new();
    archive
        .by_name("ppt/slides/slide1.xml")
        .unwrap_or_else(|error| panic!("{error}"))
        .read_to_string(&mut slide_xml)
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(slide_xml.contains("Artifacts E2E Smoke"));
    assert!(slide_xml.contains("Patched codex binary executed artifacts successfully."));
}

fn assert_success(output: &ArtifactCommandOutput) {
    assert!(output.success());
    assert_eq!(output.exit_code, Some(0));
}

fn write_installed_runtime(install_dir: &Path, runtime_version: &str) {
    write_installed_runtime_with_source(install_dir, runtime_version, "export const ok = true;\n");
}

fn write_installed_runtime_with_source(
    install_dir: &Path,
    runtime_version: &str,
    module_source: &str,
) {
    fs::create_dir_all(install_dir.join("dist")).unwrap_or_else(|error| panic!("{error}"));
    fs::write(
        install_dir.join("package.json"),
        serde_json::json!({
            "name": "@oai/artifact-tool",
            "version": runtime_version,
            "type": "module",
            "exports": {
                ".": "./dist/artifact_tool.mjs",
            }
        })
        .to_string(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    fs::write(install_dir.join("dist/artifact_tool.mjs"), module_source)
        .unwrap_or_else(|error| panic!("{error}"));
}

fn build_zip_archive(runtime_version: &str) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut bytes);
        let options = SimpleFileOptions::default();
        let package_json = serde_json::json!({
            "name": "@oai/artifact-tool",
            "version": runtime_version,
            "type": "module",
            "exports": {
                ".": "./dist/artifact_tool.mjs",
            }
        })
        .to_string()
        .into_bytes();
        zip.start_file("artifact-runtime/package.json", options)
            .unwrap_or_else(|error| panic!("{error}"));
        zip.write_all(&package_json)
            .unwrap_or_else(|error| panic!("{error}"));
        zip.start_file("artifact-runtime/dist/artifact_tool.mjs", options)
            .unwrap_or_else(|error| panic!("{error}"));
        zip.write_all(b"export const ok = true;\n")
            .unwrap_or_else(|error| panic!("{error}"));
        zip.finish().unwrap_or_else(|error| panic!("{error}"));
    }
    bytes.into_inner()
}

fn build_tar_gz_archive(runtime_version: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let encoder = GzEncoder::new(&mut bytes, Compression::default());
        let mut archive = TarBuilder::new(encoder);

        let package_json = serde_json::json!({
            "name": "@oai/artifact-tool",
            "version": runtime_version,
            "type": "module",
            "exports": {
                ".": "./dist/artifact_tool.mjs",
            }
        })
        .to_string()
        .into_bytes();
        let mut package_header = tar::Header::new_gnu();
        package_header.set_mode(0o644);
        package_header.set_size(package_json.len() as u64);
        package_header.set_cksum();
        archive
            .append_data(
                &mut package_header,
                "package/package.json",
                package_json.as_slice(),
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let build_js = b"export const ok = true;\n";
        let mut build_header = tar::Header::new_gnu();
        build_header.set_mode(0o644);
        build_header.set_size(build_js.len() as u64);
        build_header.set_cksum();
        archive
            .append_data(
                &mut build_header,
                "package/dist/artifact_tool.mjs",
                &build_js[..],
            )
            .unwrap_or_else(|error| panic!("{error}"));

        archive.finish().unwrap_or_else(|error| panic!("{error}"));
    }
    bytes
}
