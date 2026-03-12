# Apex Web Dashboard

This directory contains the Next.js frontend for the Apex dashboard.

## Directory Role

- `web/` contains frontend source code and frontend tooling configuration only
- `target/web/` is the production export output consumed by the Rust backend
- generated artifacts such as `.next/`, `out/`, `node_modules/`, and `test-results/` must not be treated as source

## Current Build Mode

The dashboard is built as a static export:

- Next.js config uses `output: "export"`
- `npm run build` exports the site and copies the result to `../target/web/`

Relevant files:

- [`package.json`](/Users/shawn/workspace/code/apex/web/package.json)
- [`next.config.ts`](/Users/shawn/workspace/code/apex/web/next.config.ts)

## Development

Install dependencies:

```bash
cd web
npm install
```

Start the frontend dev server:

```bash
npm run dev
```

This runs Next.js locally on `http://127.0.0.1:3000`.

The Rust gateway should be started separately from the repository root when frontend work needs backend APIs.

## Production Build

Build static assets for backend serving:

```bash
cd web
npm run build
```

After build, the exported files are written to:

```text
../target/web/
```

The `web/` source directory should not contain exported HTML, `_next/`, `_not-found/`, or other generated static output.

## Testing

Run frontend lint:

```bash
cd web
npm run lint
```

Run Playwright tests:

```bash
cd web
npm test
```

## Source Layout

```text
web/
├─ src/
│  ├─ app/
│  ├─ components/
│  └─ lib/
├─ public/
├─ tests/
├─ package.json
├─ next.config.ts
└─ playwright.config.ts
```

## Release Direction

The current backend serves static files from `target/web/`.

The intended release direction is:

1. keep `web/` as source only
2. keep `target/web/` as the single export directory
3. embed the exported assets into the Rust binary for unified distribution
