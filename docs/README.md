# BranchBox documentation website

User-facing documentation lives in `docs/` and is built with Docusaurus. The repository's separate `website/` directory holds the landing page. The combined deployment serves the landing page at `https://branchbox.dev/` and these docs at `https://branchbox.dev/docs/`.

## Installation

```bash
npm install
```

## Local Development

```bash
npm start
```

This command starts a local development server and opens up a browser window. Most changes are reflected live without having to restart the server.

## Build

```bash
npm run build
```

This command generates static content into the `build` directory and can be served using any static contents hosting service.

## Deployment

The **Deploy Site** GitHub Actions workflow builds and deploys changes to `main`. It copies the landing page to the deployment root and the Docusaurus output to `/docs/`.

To build the same combined layout locally, run `scripts/build-site.sh` from the repository root. This installs the locked docs dependencies and writes the combined site to the root `build/` directory.

Mac app media lives in `static/img/mac-app/` and `static/media/`. Docusaurus pages use `useBaseUrl` for media paths; the static landing page references `/docs/img/mac-app/` and `/docs/media/`. Keep captured screens free of private project paths and credentials, and label preview-backend images as illustrative sample data.
