---
"@godaddy/cli": patch
---

Hardened CLI security in three areas without changing intended workflows:

- Block extension deploy path traversal by validating `handle` and `source` stay within the extension workspace.
- Quote and escape generated `.env` values to prevent newline/comment-based env injection.
- Restrict truncation `full_output` dump permissions to owner-only (`0700` dir, `0600` files).

Also adds regression tests covering these protections.
