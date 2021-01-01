// Symphonia
// Copyright (c) 2020 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::str;

use symphonia_core::errors::{Result, decode_error};
use symphonia_core::io::{ByteStream, BufStream};
use symphonia_core::util::bits;
use symphonia_core::meta::{Metadata, MetadataBuilder, StandardTagKey, StandardVisualKey, Tag, Visual};

use crate::atoms::{Atom, AtomHeader, AtomIterator, AtomType};

#[derive(Debug)]
pub enum DataType {
    AffineTransformF64,
    Bmp,
    DimensionsF32,
    Float32,
    Float64,
    Jpeg,
    NoType,
    Png,
    PointF32,
    QuickTimeMetadata,
    RectF32,
    ShiftJis,
    SignedInt16,
    SignedInt32,
    SignedInt64,
    SignedInt8,
    SignedIntVariable,
    UnsignedInt16,
    UnsignedInt32,
    UnsignedInt64,
    UnsignedInt8,
    UnsignedIntVariable,
    Utf16,
    Utf16Sort,
    Utf8,
    Utf8Sort,
    Unknown(u32),
}

impl From<u32> for DataType {
    fn from(value: u32) -> Self {
        match value {
            0 => DataType::NoType,
            1 => DataType::Utf8,
            2 => DataType::Utf16,
            3 => DataType::ShiftJis,
            4 => DataType::Utf8Sort,
            5 => DataType::Utf16Sort,
            13 => DataType::Jpeg,
            14 => DataType::Png,
            21 => DataType::SignedIntVariable,
            22 => DataType::UnsignedIntVariable,
            23 => DataType::Float32,
            24 => DataType::Float64,
            27 => DataType::Bmp,
            28 => DataType::QuickTimeMetadata,
            65 => DataType::SignedInt8,
            66 => DataType::SignedInt16,
            67 => DataType::SignedInt32,
            70 => DataType::PointF32,
            71 => DataType::DimensionsF32,
            72 => DataType::RectF32,
            74 => DataType::SignedInt64,
            75 => DataType::UnsignedInt8,
            76 => DataType::UnsignedInt16,
            77 => DataType::UnsignedInt32,
            78 => DataType::UnsignedInt64,
            79 => DataType::AffineTransformF64,
            _  => DataType::Unknown(value),
        }
    }
}


fn add_string_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
    std_key: Option<StandardTagKey>,
) -> Result<()> {

    let tag = iter.read_atom::<MetaTagAtom>()?;

    // There should only be 1 value.
    if let Some(value) = tag.values.first() {
        builder.add_tag(Tag::new(std_key, "", str::from_utf8(&value.data).unwrap()));
    }

    Ok(())
}

fn add_var_signed_int_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
    std_key: StandardTagKey,
) -> Result<()> {

    let tag = iter.read_atom::<MetaTagAtom>()?;

    if let Some(value) = tag.values.first() {
        let len = value.data.len();

        // A variable sized big-endian signed integer may be between 1 and 4 bytes.
        if len > 0 && len <= 4 {
            let mut bs = BufStream::new(&value.data);

            // Read the appropriately sized unsigned integer.
            let unsigned = match len {
                1 => bs.read_u8()?.into(),
                2 => bs.read_be_u16()?.into(),
                3 => bs.read_be_u24()?,
                4 => bs.read_be_u32()?,
                _ => unreachable!(),
            };

            // Sign extend it.
            let signed = bits::sign_extend_leq32_to_i32(unsigned, 8 * len as u32);

            builder.add_tag(Tag::new(Some(std_key), "", &signed.to_string()));
        }
    }

    Ok(())
}

fn add_boolean_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
    std_key: StandardTagKey,
) -> Result<()> {
    
    let tag = iter.read_atom::<MetaTagAtom>()?;

    // There should only be 1 value.
    if let Some(value) = tag.values.first() {
        // Boolean tags are just "flags", only add a tag if the boolean is true (1).
        if let Some(bool_value) = value.data.first() {
            if *bool_value == 1 {
                builder.add_tag(Tag::new(Some(std_key), "", ""));

            }
        }
    }

    Ok(())
}

fn add_m_of_n_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
    m_key: StandardTagKey,
    n_key: StandardTagKey,
) -> Result<()> {

    let tag = iter.read_atom::<MetaTagAtom>()?;

    // There should only be 1 value.
    if let Some(value) = tag.values.first() {
        // The trkn and disk atoms contains an 8 byte value buffer, where the 4th and 6th bytes
        // indicate the track/disk number and total number of tracks/disks, respectively. Odd.
        if value.data.len() == 8 {
            let m_value = value.data[3];
            let n_value = value.data[5];

            builder.add_tag(Tag::new(Some(m_key), "", &m_value.to_string()));
            builder.add_tag(Tag::new(Some(n_key), "", &n_value.to_string()));
        }
    }

    Ok(())
}

fn add_visual_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
) -> Result<()> {
    
    let tag = iter.read_atom::<MetaTagAtom>()?;

    // There could be more than one attached image.
    for value in tag.values {
        let media_type = match value.data_type {
            DataType::Bmp  => "image/bmp",
            DataType::Jpeg => "image/jpeg",
            DataType::Png  => "image/png",
            _ => "",
        };

        builder.add_visual(Visual {
            media_type: media_type.into(),
            dimensions: None,
            bits_per_pixel: None,
            color_mode: None,
            usage: Some(StandardVisualKey::FrontCover),
            tags: Default::default(),
            data: value.data,
        });
    }

    Ok(())
}

fn add_advisory_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
) -> Result<()> {
    Ok(())
}

fn add_media_type_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
) -> Result<()> {
    let tag = iter.read_atom::<MetaTagAtom>()?;

    // There should only be 1 value.
    if let Some(value) = tag.values.first() {
        if let Some(media_type_value) = value.data.get(0) {
            let media_type = match media_type_value {
                0  => "Movie",
                1  => "Normal",
                2  => "Audio Book",
                5  => "Whacked Bookmark",
                6  => "Music Video",
                9  => "Short Film",
                10 => "TV Show",
                11 => "Booklet",
                _  => "Unknown",
            };

            builder.add_tag(Tag::new(Some(StandardTagKey::MediaFormat), "", media_type.into()));
        }
    }

    Ok(())
}

fn add_freeform_tag<B: ByteStream>(
    iter: &mut AtomIterator<B>,
    builder: &mut MetadataBuilder,
) -> Result<()> {

    let tag = iter.read_atom::<MetaTagAtom>()?;

    // A user-defined tag should only have 1 value.
    if let Some(value) = tag.values.first() {
        builder.add_tag(Tag::new(None, &tag.full_name(), str::from_utf8(&value.data).unwrap()));
    }

    Ok(())

}

/// Metadata tag data atom.
pub struct MetaTagDataAtom {
    /// Atom header.
    header: AtomHeader,
    /// Tag data.
    pub data: Box<[u8]>,
    /// The data type contained in buf.
    pub data_type: DataType,
}

impl Atom for MetaTagDataAtom {
    fn header(&self) -> AtomHeader {
        self.header
    }

    fn read<B: ByteStream>(reader: &mut B, header: AtomHeader) -> Result<Self> {
        let (version, flags) = AtomHeader::read_extra(reader)?;

        // For the mov brand, this a data type indicator and must always be 0 (well-known type). It
        // specifies the table in which the next 24-bit integer specifying the actual data type
        // indexes. For iso/mp4, this is a version, and there is only one version, 0. Therefore, flags
        // are interpreted as the actual data type index.
        if version != 0 {
            return decode_error("invalid data atom version");
        }

        let data_type = DataType::from(flags);

        // For the mov brand, the next four bytes are country and languages code. However, for iso/mp4
        // these codes should be ignored.
        let _country = reader.read_be_u16()?;
        let _language = reader.read_be_u16()?;

        // The data payload is the remainder of the atom.
        // TODO: Apply a limit.
        let data = reader.read_boxed_slice_exact(
            (header.data_len - AtomHeader::EXTRA_DATA_SIZE - 4) as usize
        )?;

        Ok(MetaTagDataAtom {
            header,
            data,
            data_type,
        })
    }
}

/// Metadata tag name and mean atom.
pub struct MetaTagNamespaceAtom {
    /// Atom header.
    header: AtomHeader,
    /// For 'mean' atoms, this is the key namespace. For 'name' atom, this is the key name.
    pub value: String,
}

impl Atom for MetaTagNamespaceAtom {
    fn header(&self) -> AtomHeader {
        self.header
    }

    fn read<B: ByteStream>(reader: &mut B, header: AtomHeader) -> Result<Self> {
        let (_, _) = AtomHeader::read_extra(reader)?;

        let buf = reader.read_boxed_slice_exact(
            (header.data_len - AtomHeader::EXTRA_DATA_SIZE) as usize
        )?;

        // Do a lossy conversion because metadata should not prevent the demuxer from working.
        let value = String::from_utf8_lossy(&buf).to_string();

        Ok(MetaTagNamespaceAtom {
            header,
            value,
        })
    }
}


/// A generic metadata tag atom.
pub struct MetaTagAtom {
    /// Atom header.
    header: AtomHeader,
    /// Tag value(s).
    pub values: Vec<MetaTagDataAtom>,
    /// Optional, tag key namespace.
    pub mean: Option<MetaTagNamespaceAtom>,
    /// Optional, tag key name.
    pub name: Option<MetaTagNamespaceAtom>,
}

impl MetaTagAtom {
    pub fn full_name(&self) -> String {
        let mut full_name = String::new();

        if self.mean.is_some() || self.name.is_some() {
            // full_name.push_str("----:");

            if let Some(mean) = &self.mean {
                full_name.push_str(&mean.value);
            }

            full_name.push(':');

            if let Some(name) = &self.name {
                full_name.push_str(&name.value);
            }
        }

        full_name
    }
}

impl Atom for MetaTagAtom {
    fn header(&self) -> AtomHeader {
        self.header
    }

    fn read<B: ByteStream>(reader: &mut B, header: AtomHeader) -> Result<Self> {
        let mut iter = AtomIterator::new(reader, header);

        let mut mean = None;
        let mut name = None;
        let mut values = Vec::new();

        while let Some(header) = iter.next()? {
            match header.atype {
                AtomType::MetaTagData => {
                    values.push(iter.read_atom::<MetaTagDataAtom>()?);
                }
                AtomType::MetaTagName => {
                    name = Some(iter.read_atom::<MetaTagNamespaceAtom>()?);
                }
                AtomType::MetaTagMeaning => {
                    mean = Some(iter.read_atom::<MetaTagNamespaceAtom>()?);
                }
                _ => ()
            }
        }

        Ok(MetaTagAtom {
            header,
            values,
            mean,
            name,
        })
    }
}

/// User data atom.
pub struct IlstAtom {
    /// Atom header.
    header: AtomHeader,
    /// Metadata revision.
    pub metadata: Metadata,
}

impl Atom for IlstAtom {
    fn header(&self) -> AtomHeader {
        self.header
    }

    fn read<B: ByteStream>(reader: &mut B, header: AtomHeader) -> Result<Self> {
        let mut iter = AtomIterator::new(reader, header);

        let mut mb = MetadataBuilder::new();

        while let Some(header) = iter.next()? {
            // Ignore standard atoms, check if other is a metadata atom.
            match &header.atype {
                AtomType::AdvisoryTag => {
                    add_advisory_tag(&mut iter, &mut mb)?
                }
                AtomType::AlbumArtistTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::AlbumArtist))?
                }
                AtomType::AlbumTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Album))?
                }
                AtomType::ArtistLowerTag => (),
                AtomType::ArtistTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Artist))?
                }
                AtomType::CategoryTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::PodcastCategory))?
                }
                AtomType::CommentTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Comment))?
                }
                AtomType::CompilationTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Compilation))?
                }
                AtomType::ComposerTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Composer))?
                }
                AtomType::CopyrightTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Copyright))?
                }
                AtomType::CoverTag => {
                    add_visual_tag(&mut iter, &mut mb)?
                }
                AtomType::CustomGenreTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Genre))?
                }
                AtomType::DateTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Date))?
                }
                AtomType::DescriptionTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Description))?
                }
                AtomType::DiskNumberTag => {
                    add_m_of_n_tag(
                        &mut iter,
                        &mut mb,
                        StandardTagKey::DiscNumber,
                        StandardTagKey::DiscTotal
                    )?
                }
                AtomType::EncodedByTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::EncodedBy))?
                }
                AtomType::EncoderTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Encoder))?
                }
                AtomType::GaplessPlaybackTag => {
                    // TODO: Need standard tag key for gapless playback.
                    // add_boolean_tag(&mut iter, &mut mb, )?
                }
                AtomType::GenreTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Genre))?
                }
                AtomType::GroupingTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::ContentGroup))?
                }
                AtomType::HdVideoTag => (),
                AtomType::IdentPodcastTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::IdentPodcast))?
                }
                AtomType::KeywordTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::PodcastKeywords))?
                }
                AtomType::LongDescriptionTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Description))?
                }
                AtomType::LyricsTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Lyrics))?
                }
                AtomType::MediaTypeTag => {
                    add_media_type_tag(&mut iter, &mut mb)?
                }
                AtomType::OwnerTag => {
                    add_string_tag(&mut iter, &mut mb, None)?
                }
                AtomType::PodcastTag => {
                    add_boolean_tag(&mut iter, &mut mb, StandardTagKey::Podcast)?
                }
                AtomType::PurchaseDateTag => {
                    add_string_tag(&mut iter, &mut mb, None)?
                }
                AtomType::RatingTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Rating))?
                }
                AtomType::SortAlbumArtistTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::SortAlbumArtist))?
                }
                AtomType::SortAlbumTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::SortAlbum))?
                }
                AtomType::SortArtistTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::Artist))?
                }
                AtomType::SortComposerTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::SortComposer))?
                }
                AtomType::SortNameTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::SortTrackTitle))?
                }
                AtomType::TempoTag => {
                    add_var_signed_int_tag(&mut iter, &mut mb, StandardTagKey::Bpm)?
                }
                AtomType::TrackNumberTag => {
                    add_m_of_n_tag(
                        &mut iter,
                        &mut mb,
                        StandardTagKey::TrackNumber,
                        StandardTagKey::TrackTotal
                    )?
                }
                AtomType::TrackTitleTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::TrackTitle))?
                }
                AtomType::TvEpisodeNameTag => (),
                AtomType::TvEpisodeNumberTag => (),
                AtomType::TvNetworkNameTag => (),
                AtomType::TvSeasonNumberTag => (),
                AtomType::TvShowNameTag => (),
                AtomType::UrlPodcastTag => {
                    add_string_tag(&mut iter, &mut mb, Some(StandardTagKey::UrlPodcast))?
                }
                AtomType::FreeFormTag => add_freeform_tag(&mut iter, &mut mb)?,
                _ => (),
            }
        }

        Ok(IlstAtom {
            header,
            metadata: mb.metadata(),
        })
    }
}