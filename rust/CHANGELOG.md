# Changelog

## [0.1.2](https://github.com/godaddy/cli/compare/v0.1.1...v0.1.2) (2026-07-10)


### Features

* flatten domain suggest term pricing and prefix next-actions with gddy ([#90](https://github.com/godaddy/cli/issues/90)) ([6f7a7a9](https://github.com/godaddy/cli/commit/6f7a7a9f2a13536a901f827c21e9804386f18812))


### Bug Fixes

* use non-API GitHub redirect for update checks, add --force to update apply ([#91](https://github.com/godaddy/cli/issues/91)) ([60cc92d](https://github.com/godaddy/cli/commit/60cc92d7a325b7d65350b2599e494b716bc1d222))

## [0.1.1](https://github.com/godaddy/cli/compare/v0.1.0...v0.1.1) (2026-07-09)


### Features

* add `domain operation status` to poll async domain operations ([#86](https://github.com/godaddy/cli/issues/86)) ([4ad277c](https://github.com/godaddy/cli/commit/4ad277c0b64cc2bae4707943c03b23adae195fe2))
* add gddy payments add command ([f28deb2](https://github.com/godaddy/cli/commit/f28deb21ec089a7ef19e6b0a0c2d33ebb5ea866a))
* add gddy payments add command ([42e3be6](https://github.com/godaddy/cli/commit/42e3be6e86465e8c1d0948c9f5808ed799bc099c))
* add more next-actions across commands ([#83](https://github.com/godaddy/cli/issues/83)) ([a62460b](https://github.com/godaddy/cli/commit/a62460bf3091a7db695da63848295eb9b8b1ad1c))
* add Personal Access Token (PAT) support ([#80](https://github.com/godaddy/cli/issues/80)) ([827d448](https://github.com/godaddy/cli/commit/827d4485b2e7c1f90d2b546962c0d32d21d0bfe3))
* default to prod, fix cross-platform config writes, support Windows installs ([#54](https://github.com/godaddy/cli/issues/54)) ([43ad0ad](https://github.com/godaddy/cli/commit/43ad0ade523d26ff61851047ff5fc9128c441bf0))
* DEV/TEST environments + OAuth scope step-up (DEVEX-719) ([#57](https://github.com/godaddy/cli/issues/57)) ([32cd77e](https://github.com/godaddy/cli/commit/32cd77e3ed66e3e0f183f9b8e627c2e18fb090be))
* DNS record management + domain list, with output-field discovery ([#65](https://github.com/godaddy/cli/issues/65)) ([fa66cfa](https://github.com/godaddy/cli/commit/fa66cfa34b20eff2687f1f6e84510bcc6c1dfa51))
* domain availability + suggest via spec-generated client ([#59](https://github.com/godaddy/cli/issues/59)) ([f56cd21](https://github.com/godaddy/cli/commit/f56cd21b47856ce2792a2000a6209cfc6ed7e549))
* domain purchase (v2 register) + domain get ([#70](https://github.com/godaddy/cli/issues/70)) ([96696ca](https://github.com/godaddy/cli/commit/96696ca8c99d506261f727954540dd28c414649f))
* integrate Node.js Hosting public API with new CLI ([#78](https://github.com/godaddy/cli/issues/78)) ([8877abe](https://github.com/godaddy/cli/commit/8877abe53be39da35e3e4c8a91390097530eb5a0))
* migrate domain & dns commands to the v3 Domains API ([#76](https://github.com/godaddy/cli/issues/76)) ([c39fbeb](https://github.com/godaddy/cli/commit/c39fbebdacd51033e9cf6e0b94c75c9b95be74c3))
* publish gddy alpha binary on rust-port pushes; rename binary to gddy ([#53](https://github.com/godaddy/cli/issues/53)) ([9a37bde](https://github.com/godaddy/cli/commit/9a37bdeaebea61f87a6c08c4909fefbabcd9e180))
* **release:** release-please, self-update, no-sudo installs ([#88](https://github.com/godaddy/cli/issues/88)) ([e66f990](https://github.com/godaddy/cli/commit/e66f990f4fb504df743559058ade23d4d210e32f))
* reorganize module categories and set feature-flag stages ([#87](https://github.com/godaddy/cli/issues/87)) ([7ddf2c4](https://github.com/godaddy/cli/commit/7ddf2c41f196be6a901728ca61394e61593d44ea))


### Bug Fixes

* address PR review — account_url in environments module, URL on browser failure ([e5966ed](https://github.com/godaddy/cli/commit/e5966ed70dfe4c04902be83647f4a464ef60d305))
* adopt cli-engine 0.2.0 fail-closed auth; mark local commands no_auth ([#56](https://github.com/godaddy/cli/issues/56)) ([1cc8d98](https://github.com/godaddy/cli/commit/1cc8d9878e821455c900f33d50b174953621c5b7))
* domain-purchase data formatting + comprehensive CLI help ([#75](https://github.com/godaddy/cli/issues/75)) ([5d15d6d](https://github.com/godaddy/cli/commit/5d15d6d4b499c34c68ba21854ba719af293355ee))
* payments add always returns URL, treats browser failure as non-fatal ([#72](https://github.com/godaddy/cli/issues/72)) ([07cba90](https://github.com/godaddy/cli/commit/07cba903693f322e22b9f1fc228ebc5f0ae5a404))
* request offline_access scope for refresh tokens, centralize hosting scopes ([#85](https://github.com/godaddy/cli/issues/85)) ([b6d49c7](https://github.com/godaddy/cli/commit/b6d49c7c7e0a29e8c3b9808332efe597008367ca))
* route all HTTP clients through --debug transport logger ([#81](https://github.com/godaddy/cli/issues/81)) ([d0c94a3](https://github.com/godaddy/cli/commit/d0c94a305b1792073d3a7760ce4a6ac3616ddc11))
* stop truncating agreement URLs in `domain agreements` human output ([#82](https://github.com/godaddy/cli/issues/82)) ([6cf9112](https://github.com/godaddy/cli/commit/6cf911265f063ce164aab92e16e619fb9f379ca5))
* surface failure detail on domain purchase, unify cli-engine version ([#84](https://github.com/godaddy/cli/issues/84)) ([d3a9cd1](https://github.com/godaddy/cli/commit/d3a9cd1930167f2f947987c292ed280205fae82d))


### Miscellaneous

* bump cli-engine to 0.3.4 for non-interactive scope step-up ([#73](https://github.com/godaddy/cli/issues/73)) ([206c2b3](https://github.com/godaddy/cli/commit/206c2b3277478e69d46019681e0bb53e38578943))
