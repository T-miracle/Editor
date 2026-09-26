# GPUI component completion style patch

`gpui-component` is copied from the crates.io `0.6.6` release (Apache-2.0).
The workspace uses it through `[patch.crates-io]` because that release exposes
only the completion menu width. The local changes add the optional
`CompletionMenuStyle` global, exported from `gpui_component::input`. Each field
is optional; without a registered style, the upstream menu appearance remains
unchanged. The app registers a style from its current theme and typography.

When updating GPUI Kit, compare these files with the new release before
removing or reapplying the patch:

- `src/input/mod.rs`
- `src/input/popovers/mod.rs`
- `src/input/popovers/completion_menu.rs`
