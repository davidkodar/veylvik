# The project format and its promise

From Veylvik 0.1.0 on, **every project you save opens in every later Veylvik**, looking
and behaving as it did. This page says how that is kept.

## What a project file is

A `.veylvik` file is JSON: the composition (its size, frame rate, length, nodes and
how they are wired, groups, markers and the rest) and two numbers that say how to read
it:

- `schema` is the **format** the file is in (1 for Veylvik 0.1.0).
- `saved_by` is the Veylvik that saved it (such as `"0.1.0"`).

Each node also says which **version** of its type it is (`"version": 1`).

## How older files are read

Reading a project first brings it up to date, before any other code sees it:

1. A file in an older format goes through the steps from its format to the current
   one, one after another.
2. Each node of an older version of its type goes through that type's steps to the
   current version.

The steps live in `crates/veylvik-document/src/format.rs` (`STEPS` and `NODE_STEPS`).
When the format or a node type has to change in a way older files would not read, its
number goes up and a step is added that turns the old shape into the new one. Steps are
never changed or taken out once released. Small additions, such as a new setting whose
default leaves older projects as they were, need no step: older files simply do not have
the setting.

## How the promise is checked

Each release keeps the files it saved, in `crates/veylvik-document/tests/format/<version>/`:
every node type as it writes it (`nodes.json`) and a set of complete projects (every
library preset's showcase scene, the Gold title and the demo). The tests open every one
of them, for every release, in the current Veylvik, and save them again. These files
are never edited or regenerated: a release's files are written once, when it is made
(`FORMAT_FIXTURES=1 cargo test -p veylvik-document --test format -- --ignored
write_this_versions_files`).

## Newer files in an older Veylvik

An older Veylvik cannot know what a newer one added, so it does not guess: a file in a
newer format, a node of a newer version, or a setting it does not know is refused, and
the message names the Veylvik that saved the file and the one trying to open it.

## What the promise covers

It covers project files (`.veylvik`). Files a project refers to (footage, images,
models, fonts, sounds, bakes) are read as their own formats. Rendering may improve
between versions (a better denoiser, a fixed bug), so a picture can differ slightly
while the project is the same. Effect presets (`.veyfx`), materials and preferences are
separate files with their own versions.
