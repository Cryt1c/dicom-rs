//! Collection of absolute item offsets from a DICOMDIR data set.

use std::io::Cursor;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_encoding::text::SpecificCharacterSet;
use dicom_encoding::transfer_syntax::TransferSyntaxIndex;
use dicom_parser::dataset::DataToken;
use dicom_parser::dataset::read::DataSetReader;
use dicom_transfer_syntax_registry::TransferSyntaxRegistry;

use crate::FileMetaTable;

use super::DicomDirError;

/// Locate the first byte of the DICOMDIR data set,
/// after the preamble, magic code, and file meta information.
fn data_set_start(bytes: &[u8]) -> Result<u64, DicomDirError> {
    if bytes.len() < 132 || &bytes[128..132] != b"DICM" {
        return Err(DicomDirError::InvalidDicomFile {
            reason: "missing the 128-byte preamble and DICM magic code".into(),
            backtrace: snafu::Backtrace::capture(),
        });
    }

    let mut cursor = Cursor::new(bytes);
    cursor.set_position(128);
    FileMetaTable::from_reader(&mut cursor).map_err(|source| DicomDirError::ParseMetaDataSet {
        source: Box::new(source),
        backtrace: snafu::Backtrace::capture(),
    })?;

    Ok(cursor.position())
}

/// Collect the absolute offsets of the item tags of the
/// Directory Record Sequence.
pub(crate) fn collect_directory_record_offsets(
    bytes: &[u8],
    meta: &FileMetaTable,
) -> Result<Vec<u32>, DicomDirError> {
    let data_set_start = data_set_start(bytes)?;
    let transfer_syntax_uid = meta.transfer_syntax();
    let transfer_syntax = TransferSyntaxRegistry
        .get(transfer_syntax_uid)
        .ok_or_else(|| DicomDirError::UnsupportedTransferSyntax {
            uid: transfer_syntax_uid.to_string(),
            backtrace: snafu::Backtrace::capture(),
        })?;

    let mut cursor = Cursor::new(&bytes[data_set_start as usize..]);
    let mut reader = DataSetReader::new_with_ts_cs(
        &mut cursor,
        transfer_syntax,
        SpecificCharacterSet::default(),
    )
    .map_err(|source| DicomDirError::CreateDataSetReader {
        source: Box::new(source),
        backtrace: snafu::Backtrace::capture(),
    })?;

    let mut sequence_stack: Vec<Tag> = Vec::new();
    let mut offsets = Vec::new();

    while let Some(token) = reader.next() {
        let token = token.map_err(|source| DicomDirError::ScanDataSet {
            source: Box::new(source),
            backtrace: snafu::Backtrace::capture(),
        })?;

        match token {
            DataToken::SequenceStart { tag, .. } => sequence_stack.push(tag),
            DataToken::SequenceEnd => {
                sequence_stack.pop();
            }
            DataToken::ItemStart { .. }
                if sequence_stack.last() == Some(&tags::DIRECTORY_RECORD_SEQUENCE) =>
            {
                // An item header is 4 bytes of tag plus 4 bytes of length.
                let offset = data_set_start + reader.position() - 8;
                offsets.push(offset as u32);
            }
            _ => {}
        }
    }

    Ok(offsets)
}
