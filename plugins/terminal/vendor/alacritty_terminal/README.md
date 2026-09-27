# Alacritty terminal WASM adaptation

This directory contains the emulator-only modules from `alacritty_terminal` 0.26.0 (Apache-2.0). The native PTY, polling, synchronization, and thread modules are omitted because the editor host exposes PTY and rendering through capability-checked APIs. The original project is https://github.com/alacritty/alacritty.

The local `lib.rs` exports only emulator modules. The two local additions in `term/mod.rs`, `primary_grid` and `primary_grid_mut`, let the guest save and limit the primary screen's scrollback while an alternate-screen application is active. `Row::occupied_len` in `grid/row.rs` bounds snapshot traversal to modified cells, avoiding costly scans across empty columns. No native process or PTY code is linked into the WASM component.
