//! What an image file is: its format, named by extension and confirmed by content, and the EXIF
//! orientation its pixels are stored in.

use std::path::Path;

/// The file formats the viewer opens, named by extension and confirmed by content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Tga,
    Bmp,
}

impl Format {
    /// From the extension, case-insensitive; `None` for anything else. This is what makes a file
    /// an image to the scan.
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match extension.as_str() {
            "jpg" | "jpeg" => Format::Jpeg,
            "png" => Format::Png,
            "tga" => Format::Tga,
            "bmp" => Format::Bmp,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Tga => "TGA",
            Format::Bmp => "BMP",
        }
    }

    /// From the leading signature. TGA has none, so it is never found this way.
    pub fn sniff(data: &[u8]) -> Option<Self> {
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Format::Jpeg)
        } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Format::Png)
        } else if data.starts_with(b"BM") {
            Some(Format::Bmp)
        } else {
            None
        }
    }
}

/// EXIF orientation tag values 1 to 8: how the stored pixels must be turned to stand upright.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Normal,
    MirrorHorizontal,
    Rotate180,
    MirrorVertical,
    /// Mirror horizontally, then rotate 270° clockwise.
    Transpose,
    Rotate90,
    /// Mirror horizontally, then rotate 90° clockwise.
    Transverse,
    Rotate270,
}

impl Orientation {
    /// Tag values outside 1 to 8 mean the tag is broken, so the pixels are shown as stored.
    pub fn from_exif(value: u32) -> Self {
        match value {
            2 => Orientation::MirrorHorizontal,
            3 => Orientation::Rotate180,
            4 => Orientation::MirrorVertical,
            5 => Orientation::Transpose,
            6 => Orientation::Rotate90,
            7 => Orientation::Transverse,
            8 => Orientation::Rotate270,
            _ => Orientation::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_five_extensions_are_images_in_any_case() {
        for name in ["a.jpg", "a.JPEG", "a.Png", "a.tga", "a.BMP"] {
            assert!(Format::from_path(Path::new(name)).is_some(), "{name}");
        }
        for name in ["a.gif", "a.webp", "a.jpg.txt", "jpg", "a."] {
            assert!(Format::from_path(Path::new(name)).is_none(), "{name}");
        }
    }

    #[test]
    fn broken_orientation_tags_mean_as_stored() {
        assert_eq!(Orientation::from_exif(6), Orientation::Rotate90);
        for broken in [0, 9, 65535] {
            assert_eq!(Orientation::from_exif(broken), Orientation::Normal);
        }
    }
}
