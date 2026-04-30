---
"@godaddy/cli": minor
---

Outgoing HTTP requests now carry a consistent `User-Agent` (`godaddy-cli/<version>` from the package manifest) and a per-request `X-Request-ID` (UUID v7) on REST catalog calls, GraphQL requests, OAuth token exchange, and webhook event-type discovery. Presigned S3 artifact uploads are unchanged: only the headers returned with the presigned URL are sent, so uploads remain signature-compatible.
