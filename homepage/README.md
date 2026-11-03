# Figcraft homepage

A separate, responsive public homepage inspired by the black-box visual appearance of fal.ai, adapted for Figcraft. It does not replace the editor home screen. No dependencies, handwritten JavaScript, external fonts, third-party media, or build step.

## Preview

From this directory:

```sh
python3 -m http.server 8740 --bind 127.0.0.1
```

Open `http://127.0.0.1:8740/`. To host, serve `index.html`, `styles.css`, and `pixels.svg` from the same directory.

## Editor destination

The launch buttons go to `/login`. On kubelet.cloud (`packaging/web/Dockerfile.kubelet`) this homepage is what `/` shows to anyone who isn't signed in; signed in, `/` is the editor, and nothing of the editor (its page, code or live sessions) is served without a session. Log in sends people on to the editor.

The template illustrations are original, illustrative examples. Their links go through log in to the editor, where visitors can choose the actual built-in templates; they do not promise to load the illustrated file.

## Interactions and accessibility

- Native disclosure navigation and mobile menu.
- Native radio inputs switch between Design, Prototype, and Hand off; Tab and arrow keys work without scripts.
- Native disclosure FAQs.
- Visible keyboard focus, skip link, descriptive page title, labeled navigation, decorative art hidden from screen readers, and reduced-motion support.
- Responsive layouts from 320 px upward.

## Asset provenance

`pixels.svg` and the inline SVG/CSS artwork were created for this homepage and are licensed MIT OR Apache-2.0, as is the rest of Figcraft. No fal.ai source, logos, screenshots, or image assets are included.

## Scope

This directory is independently deployable and is not included in the existing Trunk editor build. Host this homepage at your public entry point and keep the editor at its current URL, or arrange separate home/editor paths in your host configuration. No deployment configuration was changed.

## Verification — 2026-11-03

Inspected in Aside at desktop and mobile sizes. Verified no horizontal overflow at 320, 390, 768, 1024, and 1440 px; desktop dropdown and mobile navigation; all three feature panels, including arrow-key selection; FAQ expansion; successful stylesheet/SVG loads; balanced HTML, unique IDs, section targets, and labeled radio controls. `cargo metadata --no-deps` also confirms the standalone directory does not affect the Rust workspace. No Rust sources were changed by this homepage task; the full Rust CI suite was not run.
