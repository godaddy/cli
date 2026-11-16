---
summary: Start here for GoDaddy hosting, including what the hosting products share and which guide covers yours
---

# `gddy hosting` — GoDaddy hosting

`gddy hosting` creates and manages hosted apps. Each hosting product has its own guide with the full walkthrough; this page covers what the products share and points you to the right guide.

## Concepts

**App** — anything you host, whatever the product. You choose the product with `--app-type` when you create the app, and every later command takes the app's `--app-id`.

**Operation** — creating an app, and some other changes that take a while, return an operation ID straight away. Poll it with `gddy hosting operation get --operation-id <operation-id>` until its status is COMPLETED or FAILED.

**Plan** — what an app is billed to. How you get one, and when an app needs it, differs by product; the product's guide explains it.

## Pick your product

| Product | `--app-type` | Guide |
|---|---|---|
| Node.js app: upload your source, preview it, then publish | `NODEJS` | `gddy guide hosting-nodejs` |

## Commands for any product

| Command | Purpose |
|---|---|
| `hosting app list --app-type <type>` | List your apps of one product |
| `hosting app get --app-id <app-id>` | An app's details, including its URLs |
| `hosting app status --app-id <app-id>` | Runtime status of an app |
| `hosting operation get --operation-id <operation-id>` | Poll a change that takes a while |

## See also

- `gddy guide auth` — how authentication and credential storage work.
