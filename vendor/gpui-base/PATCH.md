# Local gpui-base patch

This directory contains the published `gpui-base` 0.6.6 source under its Apache-2.0 license.

The editor saves `last_bounds` after applying the document scroll offset. Using those moving
bounds to reject mouse movement causes hover requests to stop after scrolling. The local change
in `src/input/base/state.rs` checks the fixed `input_bounds` viewport instead. The package's
standalone test and benchmark targets are omitted from this dependency copy.
