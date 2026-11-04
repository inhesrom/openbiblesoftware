//! Container adapters for the locked Commentary and Dictionary editions.
use super::lock::Source;
use anyhow::{Context, Result, bail};
use obs_formats::{
    DictionaryData, ImportReport, StudyImport, import_dictionary, import_jfb_book, import_mhc,
};
use obs_model::{Manifest, Resource, ResourceType};
use obs_scripture::Book;
use std::io::{Cursor, Read};

pub(super) fn import(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let dictionary = matches!(source.importer.as_str(), "easton-zld" | "smith-rawld");
    let mut manifest = Manifest::new(
        &source.resource_id,
        &source.title,
        &source.abbreviation,
        if dictionary {
            ResourceType::Dictionary
        } else {
            ResourceType::Commentary
        },
        &source.language,
    );
    manifest.source_format.clone_from(&source.importer);
    manifest.source_file = Some(
        source
            .origin_url
            .as_deref()
            .unwrap_or(&source.url)
            .to_string(),
    );
    manifest.import_tool_version = format!("xtask starter {}", env!("CARGO_PKG_VERSION"));
    let mut import = StudyImport::new(manifest);
    if source.importer == "wikisource-mhc" {
        let mut xml = String::new();
        if bytes.starts_with(&[0x1f, 0x8b]) {
            flate2::read::GzDecoder::new(bytes).read_to_string(&mut xml)?;
        } else {
            xml = std::str::from_utf8(bytes)?.to_string();
        }
        import_mhc(&mut import, &xml)?;
    } else {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut read = |name: &str| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            zip.by_name(name)
                .with_context(|| format!("missing archive member {name}"))?
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        match source.importer.as_str() {
            "jfb-html" => {
                for (i, book) in Book::ALL.into_iter().enumerate() {
                    let raw = read(&format!("JFB{:02}.htm", i + 1))?;
                    let text: String = raw.into_iter().map(char::from).collect();
                    import_jfb_book(&mut import, book, &text)?;
                }
            }
            "easton-zld" => {
                let base = "modules/lexdict/zld/easton/easton";
                let index = read(&format!("{base}.idx"))?;
                let data = read(&format!("{base}.dat"))?;
                let block_index = read(&format!("{base}.zdx"))?;
                let blocks = read(&format!("{base}.zdt"))?;
                import_dictionary(
                    &mut import,
                    DictionaryData::Easton {
                        index: &index,
                        data: &data,
                        block_index: &block_index,
                        blocks: &blocks,
                    },
                )?;
            }
            "smith-rawld" => {
                let base = "modules/lexdict/rawld/smith/smith";
                let index = read(&format!("{base}.idx"))?;
                let data = read(&format!("{base}.dat"))?;
                import_dictionary(
                    &mut import,
                    DictionaryData::Smith {
                        index: &index,
                        data: &data,
                    },
                )?;
            }
            other => bail!("unknown study importer {other}"),
        }
    }
    import.finish()
}
