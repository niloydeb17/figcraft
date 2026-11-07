//! When the page's cover ("Loading FigCraft…", `#figcraft_loading` in `index.html`) comes off.
//!
//! The app draws its first frame long before the server has said who is signed in, so without the
//! cover a reload showed the signed-out Home (the FigCraft mark, "Your workspace", the templates),
//! then your Home, and only then the file the address names. The cover stays until the first screen
//! is the one you came for: Home with your account, its recent files and their pictures, or the
//! file from the address (`#file=` / `#join=`) once it opened or couldn't. It never stays longer
//! than [`MAX_COVER_MS`], so a slow or silent server can't keep the page hidden.

/// The longest the cover stays up after the app started, in milliseconds.
pub const MAX_COVER_MS: f64 = 6_000.0;
/// Frames drawn with everything in before the cover comes off: the first lays Home's cards out at
/// their new size, the second draws them where they belong.
const SETTLED_FRAMES: u32 = 2;

/// What the page has so far.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    /// The server answered who is signed in (or couldn't be reached), and, when someone is, their
    /// recent files and picture arrived (or failed).
    pub account: bool,
    /// Every recent file's picture that was asked for arrived (or failed).
    pub pictures: bool,
    /// A file is open in the editor.
    pub file_shown: bool,
    /// A session is being joined (the address's file, on its way).
    pub joining: bool,
}

/// Whether the page shows what it opened for.
pub fn settled(file_address: bool, p: Progress) -> bool {
    if !p.account {
        return false;
    }
    if file_address { p.file_shown || !p.joining } else { p.pictures }
}

/// The cover over the page while it loads.
#[derive(Debug)]
pub struct Cover {
    /// The address named a file to open.
    file_address: bool,
    started: f64,
    /// Frames drawn since the page settled.
    settled_frames: u32,
    off: bool,
}

impl Cover {
    pub fn new(file_address: bool, now: f64) -> Self {
        Cover { file_address, started: now, settled_frames: 0, off: false }
    }

    /// Once a frame, after it was drawn: true the one time the cover should come off.
    pub fn frame(&mut self, p: Progress, now: f64) -> bool {
        if self.off {
            return false;
        }
        if settled(self.file_address, p) {
            self.settled_frames += 1;
        } else {
            self.settled_frames = 0;
        }
        // A clock that isn't a number can't be compared: it counts as late.
        let late = (now - self.started).partial_cmp(&MAX_COVER_MS).is_none_or(|o| o.is_ge());
        self.off = self.settled_frames >= SETTLED_FRAMES || late;
        self.off
    }

    /// The cover is still up: keep frames coming so it comes off on time.
    pub fn up(&self) -> bool {
        !self.off
    }
}
