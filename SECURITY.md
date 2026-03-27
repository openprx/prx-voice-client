# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in PRX Voice Client, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email security@prx.dev (or open a private security advisory on GitHub).

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

We aim to acknowledge reports within 48 hours and provide a fix within 7 days for critical issues.

## Security Design

- Audio is only captured when explicitly activated by the user
- Credentials are stored using platform-native secure storage
- Sensitive data is sanitized in logs by default
- No telemetry is sent without explicit user consent
- All FFI boundaries validate inputs and prevent panic propagation
