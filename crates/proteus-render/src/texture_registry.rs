//! [`TextureRegistry`] — tracks GPU textures beyond the core atlases.
//!
//! The actual `wgpu::Texture` objects live inside [`crate::QuadPipeline`], which
//! has sole authority over their bind-group membership.  The registry records
//! *what* textures exist and their metadata (kind, dimensions, active/suspended
//! state).  Callers interact with the registry through the pipeline's wrapper
//! methods ([`crate::QuadPipeline::init_video`],
//! [`crate::QuadPipeline::upload_video_frame`], etc.) rather than directly.
//!
//! ## M9 — Video
//!
//! In M9 the registry holds exactly one `TextureKind::Video` slot.  The video
//! texture is referenced by `atlas_page = 2` in [`crate::QuadInstance`] and
//! sampled from the `video_atlas` binding in the fragment shader.

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Opaque handle identifying a texture registered with the [`TextureRegistry`].
pub type TextureId = u32;

/// The category of a registered texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKind {
    /// A streaming video feed — pixel data is replaced each frame via
    /// [`crate::QuadPipeline::upload_video_frame`].
    Video,
}

// ---------------------------------------------------------------------------
// TextureRegistry
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Entry {
    id: TextureId,
    kind: TextureKind,
    width: u32,
    height: u32,
    /// `false` while suspended (GPU memory released / placeholder installed).
    active: bool,
}

/// Tracks GPU texture allocations beyond the core `main_atlas` and
/// `transition_atlas` that are managed by [`crate::QuadPipeline`].
///
/// The registry is a *metadata store* only — it does not own wgpu resources.
/// GPU resources (the underlying `wgpu::Texture`) are managed by
/// [`crate::QuadPipeline`] and are created/destroyed through its methods.
#[derive(Debug, Default)]
pub struct TextureRegistry {
    entries: Vec<Entry>,
    next_id: TextureId,
}

impl TextureRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// Record a new texture allocation.  Returns the [`TextureId`] assigned to it.
    ///
    /// This is called internally by pipeline wrapper methods; external callers
    /// should use [`crate::QuadPipeline::init_video`] instead of this directly.
    pub(crate) fn register(&mut self, kind: TextureKind, width: u32, height: u32) -> TextureId {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            kind,
            width,
            height,
            active: true,
        });
        id
    }

    /// Mark the texture as suspended (GPU memory freed or replaced with placeholder).
    pub(crate) fn mark_suspended(&mut self, id: TextureId) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.active = false;
        }
    }

    /// Mark the texture as active again after a [`crate::QuadPipeline::resume_video`] call.
    pub(crate) fn mark_active(&mut self, id: TextureId) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.active = true;
        }
    }

    /// Returns `true` if the texture is registered and not currently suspended.
    pub fn is_active(&self, id: TextureId) -> bool {
        self.entries.iter().any(|e| e.id == id && e.active)
    }

    /// Returns the kind and pixel dimensions of a registered texture, if found.
    pub fn info(&self, id: TextureId) -> Option<(TextureKind, u32, u32)> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.kind, e.width, e.height))
    }
}
