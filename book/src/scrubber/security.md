# Security Vulnerability Detection

Detects common security vulnerabilities and insecure patterns frequently produced by AI code generation.

The security detector runs **regex-based rules** covering OWASP Top 10 categories: injection attacks, broken authentication, insecure cryptography, unsafe deserialization, misconfiguration, and more. Each rule targets multiple languages: Rust, Go, Zig, TypeScript/TSX, JavaScript/JSX, Python, and C#.

## Architecture

- **15 rules** (SEC001–SEC015) covering high-frequency AI security anti-patterns
- **Regex-based detection** with per-rule confidence scores (0.68–0.90)
- **Per-language filtering** — most rules target specific extensions; rules with no explicit extensions apply to all supported source languages
- **Compiled once, reused per file** — regexes cached in `LazyLock` for performance (no per-file recompilation)
- **Cross-platform path handling** — uses `Path::components()` instead of string contains for Windows compatibility

## What It Detects

### SEC001: SQL / Shell Injection

**Severity:** HIGH | **Confidence:** 0.80

User input directly interpolated into SQL or shell commands via string concatenation or formatting.

```python
# ❌ AI-generated: uses f-string without parameterized query
cursor.execute(f"SELECT * FROM users WHERE name = {name}")

# ✅ Secure: uses parameterized query
cursor.execute("SELECT * FROM users WHERE name = ?", (name,))
```

**Suggestion:** Use parameterised queries (prepared statements) for SQL. For shell commands, use an argument list API (`std::process::Command`, `subprocess.run([...])`) instead of string interpolation.

---

### SEC002: eval() / exec() with Dynamic Input

**Severity:** HIGH | **Confidence:** 0.85

Dynamic code execution via `eval()` or `exec()` with a non-literal argument. AI often defaults to `eval` instead of proper dispatch tables.

```python
# ❌ AI-generated: uses eval with user input
user_code = request.args['code']
result = eval(user_code)

# ✅ Secure: uses a dispatch table or sandboxed interpreter
HANDLERS = {'add': lambda a, b: a + b, 'sub': lambda a, b: a - b}
result = HANDLERS.get(user_code, lambda *_: None)()
```

**Suggestion:** Replace with a safe alternative: a dispatch table (dict/match), an AST validator, or a proper plugin API.

---

### SEC003: Hardcoded Secrets

**Severity:** HIGH | **Confidence:** 0.75

Hardcoded API key, password, token, or other credential.

```python
# ❌ AI-generated: literal secret in code
api_key = "sk-abc123longkeyvalue"
```

**Suggestion:** Load secrets from environment variables or a secrets manager (e.g., AWS Secrets Manager, HashiCorp Vault). Rotate any committed credentials immediately.

---

### SEC004: Weak / Broken Cryptography

**Severity:** HIGH | **Confidence:** 0.85

Use of deprecated or insecure algorithms: MD5, SHA-1, DES, 3DES, RC4, Blowfish, or ECB mode. AI training data includes many outdated examples.

```python
# ❌ AI-generated: uses MD5 for password hashing
import hashlib
h = hashlib.md5(password).hexdigest()

# ✅ Secure: uses bcrypt / scrypt / Argon2id
import bcrypt
hashed = bcrypt.hashpw(password.encode(), bcrypt.gensalt())
```

**Suggestion:** Use **SHA-256/SHA-3** for hashing, **AES-256-GCM** or **ChaCha20-Poly1305** for encryption, **Argon2id/bcrypt/scrypt** for password hashing.

---

### SEC005: TLS Verification Disabled

**Severity:** HIGH | **Confidence:** 0.90

Certificate verification disabled (`InsecureSkipVerify`, `verify=False`, etc.). This is a critical MITM vulnerability; AI disables it to bypass handshake errors during development.

```go
// ❌ AI-generated: disables certificate verification
config := &tls.Config{InsecureSkipVerify: true}
```

**Suggestion:** Never disable TLS verification in production. Fix root certificate issues instead (update CA bundle, trust additional CAs, or fix hostname mismatches).

---

### SEC006: JWT Algorithm Confusion / "alg: none"

**Severity:** HIGH | **Confidence:** 0.88

JWT with `alg: none` or algorithm confusion vulnerability. AI copies JWT examples without validating the algorithm.

```typescript
// ❌ AI-generated: accepts any or no algorithm
const decoded = jwt.decode(token);  // no algorithm check

// ✅ Secure: pins the algorithm and verifies the signature
const decoded = jwt.verify(token, secret, { algorithms: ['HS256'] });
```

**Suggestion:** Always specify and pin the expected algorithm. Reject tokens with `alg: none`. Use a well-audited library and verify the signature before trusting any claim.

---

### SEC007: dangerouslySetInnerHTML (React XSS)

**Severity:** HIGH | **Confidence:** 0.90

React `dangerouslySetInnerHTML` used with a non-constant value.

```jsx
// ❌ AI-generated: user input passed directly to dangerouslySetInnerHTML
<div dangerouslySetInnerHTML={{ __html: userContent }} />

// ✅ Secure: sanitise HTML before rendering
import DOMPurify from 'dompurify';
<div dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(userContent) }} />
```

**Suggestion:** Sanitise HTML with DOMPurify before passing to `dangerouslySetInnerHTML`, or redesign to avoid raw HTML injection entirely.

---

### SEC008: Path Traversal

**Severity:** HIGH | **Confidence:** 0.75

User-supplied input used in file path construction without sanitisation or canonicalisation.

```rust
// ❌ AI-generated: uses user input directly in path
let file_path = req.params.filename;
let content = std::fs::read_to_string(&file_path)?;

// ✅ Secure: canonicalise and check bounds
let base = std::path::Path::new("/public");
let resolved = base.join(&filename).canonicalize()?;
if !resolved.starts_with(base.canonicalize()?) {
    return Err("path traversal attempt");
}
```

**Suggestion:** Canonicalise the resolved path and assert it remains within an allowed base directory. Reject paths containing `..` components.

---

### SEC009: Credentials Written to Logs

**Severity:** MEDIUM | **Confidence:** 0.70

Possible credential or token written to logs (detected when `log`, `console.log`, `println`, etc. is called alongside keywords like `password`, `token`, `api_key`).

```python
# ❌ AI-generated: logs entire request (may contain tokens)
logger.info(f"Request received: {request.json()}")

# ✅ Secure: logs only safe fields
logger.info(f"Request from user {request.json()['user_id']} to {request.path}")
```

**Suggestion:** Redact sensitive fields before logging. Use structured logging with explicit field allow-lists rather than logging entire objects.

---

### SEC010: Debug Mode Enabled in Production

**Severity:** MEDIUM | **Confidence:** 0.72

Debug mode or verbose error details enabled (`DEBUG=True`, `app.run(..., debug=True)`, developer exception pages). AI leaves debug flags on; this leaks stack traces and internal state to clients.

```python
# ❌ AI-generated: debug mode enabled
app.run(host='0.0.0.0', debug=True)

# ✅ Production: debug off, errors suppressed
app.run(host='0.0.0.0', debug=False)
```

**Suggestion:** Set `debug=False` in production. Use generic error messages for HTTP responses; log detailed stack traces server-side only.

---

### SEC011: Non-CSPRNG for Security Values

**Severity:** HIGH | **Confidence:** 0.78

Non-cryptographic RNG used for security-sensitive values (tokens, nonces, keys, salts, OTPs). AI defaults to `Math.random()` or `random.random()` because they're simpler.

```typescript
// ❌ AI-generated: uses weak RNG for token
const token = Math.random().toString(36).slice(2);

// ✅ Secure: uses CSPRNG
const token = crypto.randomBytes(32).toString('hex');
```

**Suggestion:** Use a CSPRNG: `crypto.randomBytes()` in Node.js, `secrets` module in Python, `rand::rngs::OsRng` in Rust, `crypto/rand` in Go, `RandomNumberGenerator` in C#.

---

### SEC012: Unsafe Deserialization

**Severity:** HIGH | **Confidence:** 0.85

Unsafe deserialisation detected (`pickle.loads`, `yaml.load` without `Loader`, `BinaryFormatter` in C#, Java `ObjectInputStream`). These are known gadget chains for arbitrary code execution.

```python
# ❌ AI-generated: uses unsafe yaml.load
config = yaml.load(user_provided_yaml)

# ✅ Secure: uses safe_load
config = yaml.safe_load(user_provided_yaml)
```

**Suggestion:** For YAML use `yaml.safe_load()`. Replace Python `pickle` with JSON or MessagePack for untrusted data. Use allow-lists for C# and Java deserialization.

---

### SEC013: SSRF: Raw User URL Fetched

**Severity:** HIGH | **Confidence:** 0.72

A URL sourced from user input is fetched without an allow-list check. AI-generated proxy or webhook handlers frequently forget this step, allowing access to internal services (169.254.*, 10.*, 192.168.*, etc.).

```typescript
// ❌ AI-generated: fetches user-supplied URL without validation
const url = req.query.url;
const resp = await fetch(url);

// ✅ Secure: validates against allow-list
const ALLOWED_HOSTS = ['api.example.com', 'cdn.example.com'];
const parsed = new URL(url);
if (!ALLOWED_HOSTS.includes(parsed.hostname)) throw new Error('forbidden');
const resp = await fetch(url);
```

**Suggestion:** Validate the host against a strict allow-list. Block private/link-local ranges (169.254.*, 10.*, 172.16-31.*, 192.168.*). Use an HTTP client that does not follow redirects by default.

---

### SEC014: Hardcoded IV / Nonce

**Severity:** HIGH | **Confidence:** 0.82

Hardcoded initialisation vector (IV) or nonce. Reusing a static IV with symmetric encryption destroys semantic security; ciphertexts become deterministic.

```python
# ❌ AI-generated: hardcoded IV
cipher = AES.new(key, AES.MODE_CBC, iv=b'0000000000000000')

# ✅ Secure: generates fresh IV for each encryption
iv = os.urandom(16)
cipher = AES.new(key, AES.MODE_CBC, iv=iv)
# prepend iv to ciphertext for decryption
```

**Suggestion:** Generate a fresh random IV/nonce for every encryption operation using a CSPRNG, and prepend it to the ciphertext so it can be recovered during decryption.

---

### SEC015: TODO/FIXME in Auth/Authz Code

**Severity:** MEDIUM | **Confidence:** 0.68

TODO or FIXME comment inside authentication or authorisation logic. AI frequently stubs out security checks and marks them for later — which in practice means never.

```typescript
// ❌ AI-generated: security check stubbed with TODO
// TODO: validate the JWT token before proceeding
const user = decodeJWT(token);
```

**Suggestion:** Implement the security check now; never ship a TODO inside auth/authz code. If intentional, track it in the issue tracker and add a test that will fail until it is addressed.

---

## Configuration

The security detector runs by default. To disable it, add to `.papertowel.toml`:

```toml
[detectors]
security = false
```

## Language Support

- **Rust** (.rs)
- **Go** (.go)
- **Zig** (.zig)
- **TypeScript/TSX** (.ts, .tsx)
- **JavaScript/JSX** (.js, .jsx)
- **Python** (.py)
- **C#** (.cs)

Rules specify target languages; a rule will only match files of the appropriate type.

## Performance

- Regexes are compiled once at startup and cached in `LazyLock`
- IO errors are logged at `debug` level and gracefully handled
- Non-source files (binaries, lock files, compiled assets) are skipped
