# Design: AI Documentation Assistant for GoDaddy Developer Portal

---

## Problem Statement

The [GoDaddy Developer Portal](https://developer.godaddy.com/en) provides API reference documentation, developer guides, and CLI installation instructions. However, developers must navigate across multiple pages to find answers to specific questions — there is no way to ask a question and get an instant, contextual answer grounded in the documentation.

Leading developer platforms (Stripe, Mintlify, Fern, GitBook) now embed AI-powered chat assistants directly into their API reference docs, allowing developers to ask questions about the section they are currently reading. This is quickly becoming a baseline expectation for developer experience.

**The portal already has a dormant AI chat implementation** (`components/ai/search.tsx`, `app/api/chat/route.ts`) built on the Vercel AI SDK v5 and OpenAI GPT-4.1, but it is currently unmounted from the header and lacks RAG grounding — it answers from the LLM's general knowledge rather than from GoDaddy's documentation.

## Goals

- Reactivate and evolve the existing dormant AI chat into a production-quality, documentation-grounded assistant
- Support section-scoped context: when a developer asks a question from an API endpoint page, the assistant understands which endpoint the developer is reading
- Reduce developer time-to-answer for common integration questions
- Provide cited, grounded responses to minimize hallucination
- Leverage the existing codebase (`components/ai/`, `app/api/chat/`, Fumadocs search infrastructure) rather than building from scratch

## Non-Goals

- Replacing the existing API reference documentation
- Building a general-purpose customer support chatbot
- Executing live API calls on behalf of the developer (Phase 1)
- Handling billing, account, or non-developer support queries
- Migrating to OpenWebUI or `@godaddy/openwebui-chat` — the existing Vercel AI SDK stack is more appropriate for this public-facing site

---

## Current State of the Developer Portal

**Repository**: [`gdcorp-commerce/developer-ecosystem-documentation`](https://github.com/gdcorp-commerce/developer-ecosystem-documentation)

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | [Fumadocs](https://fumadocs.dev/) — modern docs framework on Next.js 15 |
| Runtime | React 19, Next.js 15 (App Router) |
| Content | MDX files in `content/docs/`, processed by `fumadocs-mdx` |
| API Reference | Generated from OpenAPI specs in `openapi-specs/specs/` via `fumadocs-openapi` |
| Search | Orama (client-side, `@orama/orama`) + Fumadocs built-in search |
| Styling | Tailwind CSS v4 |
| LLM Integration | Vercel AI SDK v5 (`ai`, `@ai-sdk/openai`, `@ai-sdk/react`) |
| Deployment | Dockerized, deployed via CDK (AWS) |

### Existing AI Infrastructure (Dormant)

The portal already has a partially built AI chat system that is **currently disabled** (the `AISearchTrigger` component is "dormant — not mounted in header"):

| Component | Path | Status | What It Does |
|-----------|------|--------|-------------|
| Chat UI | `components/ai/search.tsx` | Dormant | Full chat dialog: streaming, markdown rendering, `useChat` hook, `provideLinks` tool for citations |
| Chat API | `app/api/chat/route.ts` | Dormant | Edge-runtime API route using `streamText` with OpenAI GPT-4.1, `provideLinks` tool |
| Trigger | `components/ai/index.tsx` | Dormant | Lazy-loaded dialog trigger — comment says "dormant, kept with app/api/chat for potential re-enable" |
| Markdown | `components/ai/markdown-processor.ts` | Dormant | Remark-based markdown-to-JSX renderer for AI responses |
| Inkeep | `lib/chat/inkeep-qa-schema.ts` | Active | Schema definitions for `provideLinks` tool (citation links) |
| Feedback | `app/api/feedback/` + `components/feedback/` | Active | User feedback collection infrastructure |

### Other Relevant Infrastructure

| Asset | Location | Description |
|-------|----------|-------------|
| `/llms-full.txt` | `app/llms-full.txt/` | Full documentation exported in LLM-consumable format |
| `/llms.txt` | `app/llms.txt/` | Discovery index for LLM agents |
| OpenAPI specs | `openapi-specs/specs/` | Vendored OpenAPI specifications for all APIs |
| Doc generation | `scripts/generate-docs.ts` | Generates REST API reference pages from OpenAPI specs |
| Orama search | `@orama/orama` in deps | Client-side full-text search engine already integrated |
| Evaluation rubric | `evaluation/` | Competitive evaluations against Stripe, Cloudflare, etc. |
| Permission system | `lib/permissions/` | Page/section-level permission filtering |
| `processedMarkdown` | `source.config.ts` | `includeProcessedMarkdown: true` — raw markdown available at build time |

### Katana Platform (Internal Reference)

The internal Katana platform (`gdcorp-engineering/katana`) provides a separate AI chat system built on OpenWebUI with the `@godaddy/openwebui-chat` React component. While this is a proven pattern for internal tools, the developer portal's existing Vercel AI SDK integration is better suited for the public-facing use case because:

1. The portal is already a Next.js app using the Vercel AI SDK — no additional framework to integrate
2. The dormant chat UI already handles streaming, markdown, tool calling, and citations
3. OpenWebUI is internal-only (`caas.open-webui.godaddy.com`) and not designed for public traffic
4. The Vercel AI SDK's edge runtime support is better for latency on a global CDN

Katana's patterns remain valuable as reference for: tool calling design, conversation quality monitoring (AI Chat Viewer), and MCP server integration.

---

## Industry Landscape

### How Other Developer Platforms Approach This

| Platform | Feature Name | Approach | Section-Scoped | RAG | Tool Calling | Citations |
|----------|-------------|----------|:--------------:|:---:|:------------:|:---------:|
| **Stripe** | "Ask about this section" | Inline button per section opens AI panel | Yes | BM25 + kNN embedding + reranker | No | Yes |
| **Mintlify** | AI Assistant | Sidebar chat, persists across page nav | Yes | Agentic retrieval (Claude-based) | No | Yes |
| **Fern** | Ask Fern | Side panel, indexes docs + SDK code | Yes | Semantic chunking + retrieval | No | Yes |
| **ReadMe** | Ask AI | Chat embedded in docs, personalized per user | Partial | Documentation indexing | No | Yes |
| **GitBook** | GitBook Assistant | Embeddable anywhere, MCP server integration | Yes | Multi-source RAG + MCP | Yes | Yes |
| **Kapa.ai** | (Overlay) | Bolt-on retrieval layer for existing docs | No | 30+ source connectors, version-aware | No | Yes |
| **Vercel** | `/llms.txt` | Not a chatbot — provides full docs as LLM context | N/A | N/A | N/A | N/A |
| **GoDaddy (proposed)** | "Ask about this section" | Evolve dormant chat with RAG + section context | Yes | Orama (local) + OpenAI embeddings | Yes | Yes |

### Key Observations

1. **Section-scoped context is the differentiator.** Stripe, Mintlify, and Fern all inject the current page or section as context. This is the difference between a generic chatbot and a genuinely useful developer tool.

2. **RAG over documentation is table stakes.** Every platform uses some form of retrieval-augmented generation to ground responses in actual documentation.

3. **Tool calling is rare but powerful.** GitBook is the only platform that supports MCP-based tool calling. GoDaddy's dormant chat already has a `provideLinks` tool for citations — this can be extended.

4. **Citations build trust.** Every platform includes citations linking back to source documentation. GoDaddy's `provideLinks` tool already supports this pattern.

5. **GoDaddy already publishes `/llms-full.txt`.** The developer portal already provides machine-readable documentation at `developer.godaddy.com/llms-full.txt`. Combined with `processedMarkdown` (enabled in `source.config.ts`), the raw content is already accessible at build time.

### Stripe Deep Dive

Stripe's implementation is the most relevant reference. Key technical details from [Stripe's engineering blog](https://stripe.dev/blog/stripes-ai-assistant-vs-code):

- **Hybrid retrieval**: Combines BM25 keyword search (for exact terms like endpoint names) with k-nearest-neighbor embedding search (for conceptual queries)
- **Reranking**: A cross-encoder reranker re-scores retrieved chunks for relevance before they are passed to the LLM
- **Context injection**: The current API section (endpoint schema, parameters, examples) is injected alongside retrieved documentation chunks
- **Grounded generation**: The LLM is constrained to generate answers based on retrieved content, with citations linking back to specific documentation sections
- **IDE extension**: The same RAG pipeline powers the Stripe VS Code extension

---

## Proposed Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                    developer.godaddy.com (Next.js 15 / Fumadocs)        │
│                                                                          │
│  ┌──────────────────────┐    ┌───────────────────────────────────────┐  │
│  │   API Reference Page  │    │  AI Chat Dialog (components/ai/)      │  │
│  │   (Fumadocs OpenAPI)  │    │  ┌─────────────────────────────────┐  │  │
│  │                       │───▶│  │ AISearchTrigger + Search dialog │  │  │
│  │  POST /v3/domains/... │    │  │ • useChat (Vercel AI SDK v5)    │  │  │
│  │  Parameters...        │    │  │ • Streaming markdown rendering  │  │  │
│  │  Response schema...   │    │  │ • provideLinks tool (citations) │  │  │
│  └──────────────────────┘    │  │ • Section context injection     │  │  │
│                               │  └──────────────┬──────────────────┘  │  │
│                               └─────────────────┼────────────────────┘  │
│                                                  │ POST /api/chat        │
│  ┌───────────────────────────────────────────────┼───────────────────┐  │
│  │  app/api/chat/route.ts (Edge Runtime)          │                   │  │
│  │  ┌────────────────────────────────────────────┼─────────────────┐ │  │
│  │  │ 1. Receive message + section context        │                 │ │  │
│  │  │ 2. Query Orama index for relevant docs    ◀┘                 │ │  │
│  │  │ 3. Build system prompt with:                                  │ │  │
│  │  │    • Retrieved doc chunks (RAG context)                       │ │  │
│  │  │    • Current section metadata                                 │ │  │
│  │  │    • Grounding instructions                                   │ │  │
│  │  │ 4. streamText() → OpenAI GPT-4.1                             │ │  │
│  │  │ 5. Stream response with provideLinks citations                │ │  │
│  │  └───────────────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  Build-time Index Generation (scripts/)                          │   │
│  │  • Parse content/docs/**/*.mdx (processedMarkdown)               │   │
│  │  • Parse openapi-specs/specs/**/*.json                           │   │
│  │  • Chunk + embed → Orama index or pre-built JSON                 │   │
│  │  • Output: static search index deployed with the app             │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────┘
```

### Data Flow

1. **Developer clicks "Ask about this section"** on an API reference page
2. **Chat dialog opens** (existing `AISearchTrigger` → `Search` component). The current page's section metadata (endpoint path, method, description, parameters) is captured and sent alongside the question
3. **`app/api/chat/route.ts`** receives the message plus section context:
   - Queries the pre-built doc index (Orama or static JSON) for relevant documentation chunks
   - Constructs a system prompt with: grounding instructions, retrieved doc chunks, and section-scoped metadata
   - Calls `streamText()` with OpenAI GPT-4.1 and the `provideLinks` tool
4. **Response streams** back via the Vercel AI SDK to the chat dialog with inline citations

### Context Scoping

The key enhancement is section-scoped context. The `useChat` transport already sends to `/api/chat` — the route needs to accept and use page context:

```typescript
// In the chat API route — enhanced system prompt
const systemPrompt = `You are a GoDaddy API documentation assistant.
Answer based ONLY on the provided documentation context.
Always cite sources using the provideLinks tool.

## Current Section
The developer is viewing: ${sectionContext.method} ${sectionContext.path}
Description: ${sectionContext.summary}

## Retrieved Documentation
${retrievedChunks.map(c => c.content).join('\n\n')}

If you cannot answer from the provided context, say so.`;
```

---

## What Needs to Change

### 1. Reactivate the Dormant Chat UI

The `AISearchTrigger` component (`components/ai/index.tsx`) is already implemented but not mounted. Changes needed:

- **Mount the trigger** in the docs layout (e.g., header or floating button)
- **Add section context** — pass the current page's metadata (title, path, OpenAPI endpoint info) to the chat component
- **Add an "Ask about this section" button** on API reference pages specifically

### 2. Add RAG to the Chat API Route

The existing `app/api/chat/route.ts` calls OpenAI directly with no documentation context. The key enhancement:

- **Build a documentation index** at build time from MDX content (`processedMarkdown` is already enabled in `source.config.ts`) and OpenAPI specs
- **Query the index** on each chat request to retrieve relevant documentation chunks
- **Inject chunks into the system prompt** so the LLM answers from documentation, not general knowledge

Two approaches for the index:

| Approach | Pros | Cons |
|----------|------|------|
| **Orama (already in deps)** | Zero new dependencies, runs in-process, fast text search | No native vector/embedding search; limited to keyword matching |
| **OpenAI embeddings + static JSON index** | Semantic search quality matches Stripe's approach | Adds build-time embedding cost; need to store/load vector index |

**Recommendation**: Start with Orama for keyword-based RAG (already a dependency), then layer in OpenAI embeddings for semantic search quality.

### 3. Enhance the `provideLinks` Tool

The existing `provideLinks` tool schema already supports citations. Enhance it to:

- Include the source page URL for each cited chunk
- Link back to the specific documentation section
- Show citation cards in the chat UI (already partially implemented in `search.tsx`)

### 4. Add a Feedback Loop

The portal already has `app/api/feedback/` and `components/feedback/` infrastructure. Wire it to the AI chat:

- Thumbs up/down on each AI response
- "Wrong answer" reporting
- Analytics on common questions that the AI struggles with (doc gap detection)

---

## Key Decisions

| Decision | Recommendation | Rationale |
|----------|---------------|-----------|
| **LLM provider** | OpenAI GPT-4.1 (already configured) | The dormant chat already uses this. No migration needed. The Vercel AI SDK supports swapping providers later. |
| **RAG index** | Orama (build-time, keyword) → OpenAI embeddings (semantic) | Orama is already a dependency. Start simple, add embeddings when keyword matching proves insufficient. |
| **Chat UI framework** | Keep existing Vercel AI SDK implementation | Already built, already uses `useChat`, streaming, tool calling. No reason to switch to `@godaddy/openwebui-chat`. |
| **Authentication** | Anonymous for read-only chat | The developer portal is public. Rate-limit by session. Add optional login later for personalized API key injection. |
| **Hosting** | Same-origin `/api/chat` route (already exists) | No additional service needed. Edge runtime provides low latency globally. |
| **Build vs. Buy** | Evolve existing code | The hardest parts (chat UI, streaming, tool calling, markdown rendering) are already built. Only RAG and section context are missing. |

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Hallucinated API examples** | Developers follow incorrect integration advice | RAG grounding constrains responses to retrieved docs. System prompt instructs the model to cite sources and say "I don't know" when unsure. |
| **Cost at scale** | OpenAI API costs with high public traffic | Rate-limit by session/IP. Start with GPT-4.1-mini for lower cost. Monitor usage analytics for forecasting. |
| **Stale index** | Documentation changes not reflected in chat answers | Index rebuilds every deployment (build-time). Fumadocs already rebuilds content on each `next build`. |
| **Prompt injection** | Adversarial users attempt to extract system prompts or misuse | System prompt hardening. Input validation. Response filtering. Edge runtime isolation. |
| **Orama index size** | Large documentation corpus may slow page load if index is client-side | Keep index server-side in the API route. Orama supports both client and server usage. |

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Developer adoption | 10% of developer portal visitors use the chat within 3 months | Analytics event on chat open / message sent |
| Answer quality | 80%+ positive feedback (thumbs up) | Existing feedback infrastructure |
| Deflection rate | 20% reduction in developer support tickets for API integration questions | Compare support ticket volume before/after launch |
| Response grounding | 90%+ of responses include at least one citation | Automated analysis via `provideLinks` tool call rate |
| Latency | First token < 2 seconds, full response < 10 seconds | Edge runtime metrics + client-side timing |

---

## Open Questions

1. **Why was the chat disabled?** The `AISearchTrigger` comment says "dormant — kept with app/api/chat for potential re-enable." Was there a quality issue, cost concern, or product decision? Understanding this is critical before reactivating.
2. **Orama vs. external vector DB**: Is Orama's keyword-based search sufficient for initial quality, or should we go straight to OpenAI embeddings?
3. **Content scope**: Should the assistant cover only the Domains API (current public scope) or prepare for broader API coverage (Commerce, Hosting, etc.)?
4. **Inkeep integration**: The `.env.example` includes `INKEEP_API_KEY` and `lib/chat/inkeep-qa-schema.ts` exists. Was Inkeep evaluated as a managed RAG alternative? Should it be reconsidered?
5. **Feedback loop**: How should user feedback feed back into documentation improvements? Direct integration with the docs repo, or via the existing evaluation rubric process?
