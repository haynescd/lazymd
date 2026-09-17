//! Image loading and sizing for terminal rendering.
//!
//! Images are scaled to fit the available width, keeping their aspect ratio,
//! using the terminal's real cell size from the [`Picker`]. Small images keep
//! their natural size rather than being blown up.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use ratatui::layout::Size;
use ratatui_image::{Resize, picker::Picker, sliced::SlicedProtocol};

/// An image ready to be rendered by [`ratatui_image`].
///
/// The [`SlicedProtocol`] lives across frames (Kitty maintains a file
/// descriptor, Sixel pre-encodes data). The height tells the layout how many
/// terminal rows the image occupies.
pub struct Image {
    /// The protocol holding the actual image data. Lives across frames.
    pub protocol: SlicedProtocol,
    /// Height in terminal cells. Used for layout.
    pub height: u16,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ImageKey {
    Path(PathBuf),
    Url(String),
}

#[derive(Clone, Debug)]
pub struct ImageDescriptor {
    pub key: ImageKey,
    pub width: usize,
}

// `SlicedProtocol` isn't `Debug`; its size is the useful part anyway.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ImageResolver {
    base_dir: PathBuf,
}

impl ImageResolver {
    pub fn new(md_path: &Path) -> Self {
        ImageResolver {
            base_dir: md_path.parent().unwrap_or(Path::new("")).to_path_buf(),
        }
    }
    pub fn resolve(&self, url: &str, width: usize) -> Option<ImageDescriptor> {
        if url.contains("://") {
            return None;
        }
        let path = self.base_dir.join(url);

        if !path.is_file() {
            return None;
        }

        Some(ImageDescriptor {
            key: ImageKey::Path(path),
            width,
        })
    }
}

/// Loads the images a document refers to. Built once per app: the [`Picker`]
/// has to query the terminal before anything else reads from stdin.
#[derive(Debug)]
pub struct ImageLoader {
    picker: Picker,
    /// Image paths in Markdown are relative to the file, not the working directory.
    base_dir: PathBuf,
}

impl ImageLoader {
    pub fn new(picker: Picker, md_path: &Path) -> Self {
        ImageLoader {
            picker,
            base_dir: md_path.parent().unwrap_or(Path::new("")).to_path_buf(),
        }
    }

    /// Loads the image at `url` and prepares it for rendering at most `width`
    /// columns wide. `None` if it's remote or can't be read or decoded.
    pub fn load(&self, url: ImageKey, width: usize) -> Option<Image> {
        let path = match url {
            ImageKey::Path(p) => p,
            _ => todo!(),
        };

        let dyn_img = image::ImageReader::open(&path).ok()?.decode().ok()?;

        let available = Size::new(u16::try_from(width).unwrap_or(u16::MAX), u16::MAX);
        let size = Resize::Fit(None).size_for(&dyn_img, self.picker.font_size(), available);
        let protocol = SlicedProtocol::new(&self.picker, dyn_img, Some(size)).ok()?;
        let height = protocol.size().height;

        Some(Image { protocol, height })
    }
}
