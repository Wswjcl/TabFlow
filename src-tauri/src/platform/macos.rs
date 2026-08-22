use super::TrackedItem;

/// Placeholder for macOS implementation.
/// Will be implemented when expanding to macOS support.
pub fn enumerate_windows() -> Vec<TrackedItem> {
    // TODO: Implement using CGWindowListCopyWindowInfo
    // - Get window list from Quartz Window Services
    // - Filter by kCGWindowLayer = 0 (normal windows)
    // - Extract window title, owner name, PID
    // - For browser tabs: use accessibility API (AXUIElement)
    // - For Finder: use NSWorkspace + NSAppleScript or accessibility
    vec![]
}