# GoDaddy CLI

A powerful command-line interface for interacting with GoDaddy's developer ecosystem. Manage your applications, handle authentication, and work with webhooks effortlessly.

## Installation

```bash
# Install the CLI globally from npm
npm install -g @godaddy/cli

# Verify installation
godaddy --help
```

## Development

```bash
# Watch mode for development using tsx
pnpm tsx --watch index.ts

# Quick command execution during development
pnpm tsx src/index.tsx application <command>
```

## Features

- **Application Management**: Create, view, and release applications
- **Authentication**: Secure OAuth-based authentication with GoDaddy
- **Webhook Management**: List available webhook event types
- **Environment Management**: Work across different GoDaddy environments
- **Actions Management**: List and describe available application actions

## Command Reference

### Global Options

```bash
godaddy --help                       # Display help information
godaddy --version                    # Display version information
godaddy -e, --env <environment>      # Set target environment (ote, prod)
godaddy --debug                      # Enable debug logging for HTTP requests and responses
```

### Environment Commands

```bash
# List all available environments
godaddy env list
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Get current environment details
godaddy env get
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Switch to a different environment
godaddy env set <environment>        # <environment> is one of: ote, prod
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# View detailed environment configuration
godaddy env info [environment]       # [environment] is one of: ote, prod (defaults to current)
  Options:
    -o, --output <format>            # Output format: json or text (default: text)
```

### Authentication Commands

```bash
# Login to GoDaddy Developer Platform
godaddy auth login                   # Opens browser for OAuth authentication
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Logout and clear stored credentials
godaddy auth logout
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Check current authentication status
godaddy auth status
  Options:
    -o, --output <format>            # Output format: json or text (default: text)
```

### Application Commands

> **Note**: `godaddy app` can be used as a shorthand alias for `godaddy application`

```bash
# List all applications
godaddy application list             # Alias: godaddy app ls
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Show application information
godaddy application info <name>      # Shows info for the named application
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Initialize a new application
godaddy application init
  Options:
    --name <name>                    # Application name
    --description <description>      # Application description
    --url <url>                      # Application URL
    --proxy-url <proxyUrl>           # Proxy URL for API endpoints
    --scopes <scopes>                # Authorization scopes (space-separated)
    -c, --config <path>              # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)

# Validate application configuration
godaddy application validate <name>
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Update existing application
godaddy application update <name>
  Options:
    --label <label>                  # Application label
    --description <description>      # Application description
    --status <status>                # Application status (ACTIVE|INACTIVE)
    -o, --output <format>            # Output format: json or text (default: text)

# Create a new release of your application
godaddy application release <name>
  Options:
    --release-version <version>      # Release version (required)
    --description <description>      # Release description
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)

# Deploy your application to the platform
godaddy application deploy <name>
  Options:
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)

# Enable an application on a store
godaddy application enable <name>
  Options:
    --store-id <storeId>             # Store ID (required)
    -o, --output <format>            # Output format: json or text (default: text)

# Disable an application on a store
godaddy application disable <name>
  Options:
    --store-id <storeId>             # Store ID (required)
    -o, --output <format>            # Output format: json or text (default: text)

# Archive an application
godaddy application archive <name>
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Add components to your application
godaddy application add <type>
  <type> can be one of:              # action, subscription, extension
```

#### Adding Actions

```bash
godaddy application add action
  Options:
    --name <name>                    # Action name (required)
    --url <url>                      # Action endpoint URL (required)
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)
```

#### Adding Subscriptions

```bash
godaddy application add subscription
  Options:
    --name <name>                    # Subscription name (required)
    --events <events>                # Comma-separated list of events (required)
    --url <url>                      # Webhook endpoint URL (required)
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)
```

#### Adding Extensions

```bash
# Add an embed extension (injected UI at specific page locations)
godaddy application add extension embed
  Options:
    --name <name>                    # Extension name (required)
    --handle <handle>                # Extension handle/unique identifier (required)
    --source <source>                # Path to extension source file (required)
    --target <targets>               # Comma-separated list of target locations (required)
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)

# Add a checkout extension (checkout flow UI)
godaddy application add extension checkout
  Options:
    --name <name>                    # Extension name (required)
    --handle <handle>                # Extension handle/unique identifier (required)
    --source <source>                # Path to extension source file (required)
    --target <targets>               # Comma-separated list of checkout target locations (required)
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)

# Set the blocks extension source (consolidated UI blocks package)
godaddy application add extension blocks
  Options:
    --source <source>                # Path to blocks extension source file (required)
    --config <path>                  # Path to configuration file
    --environment <env>              # Environment (ote|prod)
    -o, --output <format>            # Output format: json or text (default: text)
```

### Webhook Commands

```bash
# List available webhook event types
godaddy webhook events               # Lists all available webhook event types you can subscribe to
  Options:
    -o, --output <format>            # Output format: json or text (default: text)
```

### Actions Commands

```bash
# List all available actions
godaddy actions list                 # Lists all available actions an application can hook into
  Options:
    -o, --output <format>            # Output format: json or text (default: text)

# Show detailed interface information for a specific action
godaddy actions describe <action>    # Displays request/response schemas for the action
  Options:
    -o, --output <format>            # Output format: json or text (default: text)
```

#### Available Actions

The following actions are available for applications to hook into:

- `location.address.verify` - Verify and standardize a physical address
- `commerce.taxes.calculate` - Calculate taxes for a purchase
- `commerce.shipping-rates.calculate` - Calculate shipping rates
- `commerce.price-adjustment.apply` - Apply price adjustments
- `commerce.price-adjustment.list` - List price adjustments
- `notifications.email.send` - Send email notifications
- `commerce.payment.get` - Get payment details
- `commerce.payment.cancel` - Cancel a payment
- `commerce.payment.refund` - Refund a payment
- `commerce.payment.process` - Process a payment
- `commerce.payment.auth` - Authorize a payment

## Automation Examples

### Complete Application Setup

Create and configure an application without interactive prompts:

```bash
# Create application
godaddy application init \
  --name "my-ecommerce-app" \
  --description "Advanced e-commerce integration" \
  --url "https://app.mystore.com" \
  --proxy-url "https://api.mystore.com" \
  --scopes "domains orders customers" \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json \
  --env prod

# Add action
godaddy application add action \
  --name "order.completed" \
  --url "https://api.mystore.com/actions/order-completed" \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json

# Add webhook subscription
godaddy application add subscription \
  --name "order-events" \
  --events "order.created,order.completed,order.cancelled" \
  --url "https://api.mystore.com/webhooks/orders" \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json

# Add embed extension
godaddy application add extension embed \
  --name "my-widget" \
  --handle "my-widget-handle" \
  --source "./extensions/embed/index.tsx" \
  --target "body.end" \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json

# Create a release
godaddy application release my-ecommerce-app \
  --release-version "1.0.0" \
  --description "Initial release" \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json

# Deploy the application
godaddy application deploy my-ecommerce-app \
  --config ./config/godaddy.prod.toml \
  --environment prod \
  --output json

# Enable on a store
godaddy application enable my-ecommerce-app \
  --store-id "12345" \
  --output json
```

## Environment Management

The CLI supports multiple GoDaddy environments:

- **ote**: Pre-production environment that mirrors production
- **prod**: Production environment for live applications

You can specify the environment in two ways:

1. Using the global `-e, --env` flag with any command: `godaddy application info my-app --env ote`
2. Setting a default environment: `godaddy env set prod`

Use `godaddy env info` to view detailed configuration for your current environment.

## Application Configuration

The CLI uses a configuration file (`godaddy.toml`) to store your application settings. You can provide a custom configuration file path using the `--config` option with commands that support it.

Environment-specific configuration files can be used by naming them `godaddy.<environment>.toml` (e.g., `godaddy.dev.toml`).

Example configuration:

```toml
name = "my-app"
client_id = "your-client-id"
description = "My GoDaddy Application"
url = "https://myapp.example.com"
proxy_url = "https://proxy.example.com"
authorization_scopes = ["domains", "shopper"]
version = "0.0.0"
actions = []

[[subscriptions.webhook]]
name = "domain-events"
events = ["example:v1:domain:created", "example:v1:domain:updated"]
url = "https://myapp.example.com/webhooks"

[extensions]
ui_extension = "value"

[dependencies]
app = [{name = "required-app", version = "^1.0.0"}]
feature = [{name = "required-feature"}]
```

## Application Deployment

The deployment process consists of several steps:

1. **Initialize**: Create your application with `godaddy application init`
2. **Configure**: Add components with the `application add` commands
3. **Validate**: Ensure your configuration is valid with `godaddy application validate`
4. **Release**: Create a new version with `godaddy application release`
5. **Deploy**: Deploy your application with `godaddy application deploy`
6. **Enable**: Enable your application on stores with `godaddy application enable`

## Authentication

Authentication is handled securely using OAuth. The CLI will:

1. Open a browser for authentication with GoDaddy
2. Store tokens securely in your system keychain
3. Automatically use the stored token for future commands

## Requirements

- Node.js 16+
- Access to GoDaddy Developer Account

## License

Copyright GoDaddy Inc. All rights reserved.
