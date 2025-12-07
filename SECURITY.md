# Security Policy

## Supported Versions

We release security updates for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.5.x   | :white_check_mark: |
| < 0.5   | :x:                |

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in mcp-execution, please report it responsibly:

### How to Report

1. **Email**: Send details to **k05h31@gmail.com**
   - Subject: `[SECURITY] mcp-execution vulnerability`
   - Include:
     - Description of the vulnerability
     - Steps to reproduce
     - Potential impact
     - Suggested fix (if any)

2. **GitHub Security Advisory** (preferred):
   - Go to https://github.com/bug-ops/mcp-execution/security/advisories/new
   - Fill out the security advisory form
   - Our team will be notified privately

### What to Expect

- **Initial Response**: Within 48 hours
- **Triage**: Within 7 days (assessment of severity and impact)
- **Fix Development**: Depends on severity
  - Critical: 1-3 days
  - High: 7-14 days
  - Medium: 14-30 days
  - Low: Next regular release

### Disclosure Policy

We follow **coordinated disclosure**:

1. You report the vulnerability privately
2. We confirm receipt and assess severity
3. We develop and test a fix
4. We release a patched version
5. We publish a security advisory (crediting you, if desired)
6. You may publicly disclose 7 days after the patch release

### Security Update Process

When a security vulnerability is confirmed:

1. **Patch Development**:
   - Create fix in private branch
   - Add regression tests
   - Review by multiple maintainers

2. **Release**:
   - Version bump (patch for fixes, minor for features)
   - Update CHANGELOG.md with security notice
   - Publish to crates.io
   - Create GitHub release with security tag

3. **Notification**:
   - Publish GitHub Security Advisory
   - Notify users via repository watch notifications
   - Update documentation

## Security Best Practices for Users

### When Using mcp-execution

1. **Keep Dependencies Updated**:
   ```bash
   cargo update
   cargo outdated  # Check for newer versions
   ```

2. **Validate Server Configurations**:
   - Only run trusted MCP servers
   - Review server command and arguments before execution
   - Avoid passing secrets as command-line arguments (use environment variables)

3. **Protect Configuration Files**:
   ```bash
   # Set restrictive permissions on config files
   chmod 600 ~/.claude/mcp.json
   ```

4. **Review Generated Code**:
   - Inspect generated TypeScript before execution
   - Use `--output` to review files before deployment

5. **Monitor Logs**:
   - Enable `--verbose` mode to see security-relevant events
   - Review logs for unusual server behavior

### For Contributors

1. **Security-Focused Development**:
   - Follow [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/)
   - Never use `unsafe` without documented rationale and review
   - Validate all external inputs (arguments, JSON schemas, file paths)
   - Use `cargo clippy` and `cargo deny` before committing

2. **Testing**:
   - Write tests for security-critical code paths
   - Include negative tests (malformed input, attack scenarios)
   - Run full test suite: `cargo nextest run --workspace`

3. **Dependencies**:
   - Check dependency versions via Context7 MCP before adding
   - Run `cargo deny check` to verify security and licenses
   - Prefer well-maintained crates with >1M downloads

## Security Features

mcp-execution implements defense-in-depth security:

### Command Injection Prevention

- Validates all server commands and arguments for shell metacharacters
- Blocks dangerous environment variables (`LD_PRELOAD`, `DYLD_*`, `PATH`)
- Uses parameterized subprocess execution (no shell interpretation)
- See: `crates/mcp-core/src/command.rs`

### Path Traversal Protection

- Validates all file paths for directory traversal attempts
- Blocks paths containing `..`
- Canonicalizes base paths before file operations
- See: `crates/mcp-files/src/builder.rs`

### Input Validation

- JSON schema parsing is read-only (no code execution)
- Template engine uses strict mode (fails on missing variables)
- CLI arguments parsed with type-safe `clap` library
- All user inputs validated before use

### Memory Safety

- **Zero unsafe code**: All crates enforce `#![deny(unsafe_code)]`
- Strong type system prevents common bugs
- Rust's ownership model eliminates use-after-free, data races

### Dependency Security

- All dependencies scanned for known vulnerabilities
- Regular updates with `cargo-deny` checks
- License compliance enforcement

## Known Security Limitations

### By Design

1. **Local Execution Only**:
   - mcp-execution spawns local processes (MCP servers)
   - Trusts that server binaries are not malicious
   - **Recommendation**: Only run servers from trusted sources

2. **Configuration File Security**:
   - `~/.claude/mcp.json` may contain API tokens
   - File permissions not enforced by CLI (user responsibility)
   - **Recommendation**: Use `chmod 600` on config files

3. **Generated Code Execution**:
   - Generated TypeScript is executed by Node.js
   - No sandboxing of generated code
   - **Recommendation**: Review generated code before use

### Future Improvements

- [ ] Add WASM sandbox for tool execution
- [ ] Implement server signature verification
- [ ] Add config file encryption option
- [ ] Runtime security policy enforcement

## External Security Resources

- **RustSec Advisory Database**: https://rustsec.org/
- **Rust Security Working Group**: https://www.rust-lang.org/governance/wgs/wg-security
- **OWASP Rust Security**: https://owasp.org/www-project-rust/
- **CVE Database**: https://cve.mitre.org/

## Recognition

We appreciate security researchers who responsibly disclose vulnerabilities. With your permission, we will credit you in:
- Security advisory
- CHANGELOG.md
- This SECURITY.md file

## Contact

- **Security Issues**: k05h31@gmail.com (private)
- **General Issues**: https://github.com/bug-ops/mcp-execution/issues (public)
- **Discussions**: https://github.com/bug-ops/mcp-execution/discussions

---

**Thank you for helping keep mcp-execution secure!**
