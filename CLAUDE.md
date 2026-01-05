# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# GoDaddy CLI Development Guide

## Commands
- **Install**: `pnpm install`
- **Build**: `pnpm run build`
- **Build (dev mode)**: `pnpm run build:dev`
- **Format**: `pnpm run format`
- **Lint**: `pnpm run lint`
- **Type Check**: `pnpm run check`
- **Run CLI**: `node ./dist/index.mjs` or `./dist/index.mjs` (after building)
- **Development Mode**: `pnpm tsx --watch index.ts`
- **Quick CLI Command**: `pnpm tsx src/index.tsx application <command>`

## Architecture

GoDaddy CLI is a terminal-based application built using:
- **React and Ink**: Terminal UI rendering with React components
- **Commander.js**: Command definition and routing
- **@clack/prompts**: Interactive user prompts and input handling
- **graphql-request/TanStack Query**: Data fetching with GraphQL
- **arktype**: Runtime type validation

### Core Architecture Components:

1. **Entry Point** (`src/index.tsx`): Defines the CLI command structure using Commander.js and renders commands with Ink

2. **Command Structure**:
   - Root commands (application, webhook, auth, env)
   - Subcommands (init, info, release, update, etc.)
   - Commands are rendered as React components

3. **Component Organization**:
   - **Commands** (`src/cmds/`): React components that implement specific CLI commands
   - **Services** (`src/services/`): Business logic, API interactions, and data handling
   - **Components** (`src/components/`): Reusable UI components
   - **Utils**: Helper functions and utilities

4. **Context Providers**:
   - Environment context for handling different deployment environments
   - Help context for managing command help displays
   - React Query for data fetching and state management

5. **Build System**:
   - ESBuild for bundling with production/development modes
   - Custom plugins for handling compatibility issues

## Code Style

- **TypeScript**: Strict mode enabled with noEmit. Use explicit types.
- **Formatting**: 
  - Tab indentation (via Biome)
  - Double quotes for strings
  - ES modules with explicit imports
- **Naming**:
  - React components: PascalCase
  - Functions/variables: camelCase
  - Files: kebab-case.tsx
- **Components**: Use functional React components with hooks.
- **Error Handling**: Use typed validation with arktype for user inputs.
- **Prompts**: Use @clack/prompts for interactive user input.

## Key Features and Concepts

### Authentication
- OAuth-based authentication with GoDaddy APIs
- Secure token storage using keytar
- Token retrieval and refresh handled by auth service

### Configuration
- Application settings stored in TOML format (godaddy.toml)
- Environment-specific configurations (ote, prod)

### Application Management
- Create, view, update, and release GoDaddy applications
- Application enablement and disablement on entities
- Component addition (actions, subscriptions, extensions)

### Webhooks
- Event subscription and management
- Event type discovery

## Development Guidelines

- When creating a new command always render the ink component. You should never have a new command without a corresponding component.