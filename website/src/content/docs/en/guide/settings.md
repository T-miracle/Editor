---
title: Settings and themes
description: Which settings apply to you, which belong to a project, and how the theme is chosen.
section: guide
order: 5
alternate: /zh-cn/guide/settings/
---

# Settings and themes

Settings open in their own window, with the sections listed on the left. Three are relevant to
a reader of this guide: appearance and behaviour, languages, and plugins.

## Appearance and behaviour

- **Theme.** Choose the light or the dark theme. `Ctrl+Alt+T` switches between them without
  opening this window. The theme styles the whole window, including plugin panels, so a plugin
  does not need its own colour settings.
- **Font sizes.** The interface and editor font sizes are set here and apply as you change them.

Settings you change here are yours. They are stored in your user data directory, not in the
project, so they follow you between projects and never enter a repository.

## Languages

Use **File associations** to add a file extension and choose an installed language. A leading
dot is optional and case is ignored. The change applies to already-open files. Remove an
association to return to plugin recognition. If its language provider is disabled or removed,
the association remains saved and the file is shown as plain text until that language is available.
These are user settings; choosing an association does not enable a plugin or change workspace trust.

The languages section lists the language providers the installed plugins contribute, separately
for recognition (which language a file is) and highlighting. A provider can be chosen
explicitly, and a project may have its own confirmed choice that overrides yours.

When only one provider exists it is used automatically. Adding a competitor does not silently
change which one is in use. Removing the chosen provider falls back to a single remaining
candidate, asks you when several remain, or reports that files are shown as plain text when
none remain.

## Plugins

The plugins section is where installed packages are managed: which are enabled globally, which
are enabled for the current project, their settings and their diagnostics. Installing,
updating and removing plugins is covered in the plugin guide for this site.

## Project settings never override yours

A project can carry its own configuration, but it cannot change your theme, your interface
language or your trust in a workspace. Project-level values only apply where the editor asks
you to confirm them, and they never grant a plugin a permission you did not approve.
