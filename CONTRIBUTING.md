# Contributing to Latte

Thank you for your interest in contributing to Latte! This document provides guidelines and information for contributors.

## 🚀 Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) installed via rustup
- [Zed Editor](https://zed.dev/) installed
- Basic knowledge of Frappe Framework and Zed extensions
- Git for version control

### Development Environment Setup

1. **Fork and Clone**
   ```bash
   git clone https://github.com/your-username/zed-latte.git
   cd zed-latte/latte
   ```

2. **Install Rust Target**
   ```bash
   rustup target add wasm32-wasip2
   ```

3. **Test the Extension**
   - Open Zed
   - Command Palette → `zed: install dev extension`
   - Select the `latte` directory
   - Test in a Frappe project

## 📁 Project Structure

```
latte/
├── extension.toml          # Extension manifest
├── Cargo.toml             # Rust project config
├── src/
│   └── lib.rs            # Main extension code
├── snippets/             # Code snippets
│   ├── python.json       # Python/Frappe snippets
│   ├── javascript.json   # JS/Client script snippets
│   └── gitignore.json    # Gitignore templates
├── languages/            # Language definitions
│   └── frappe-python/    # Enhanced Python support
└── README.md
```

## 🔧 Development Guidelines

### Code Style

#### Rust Code
- Follow Rust standard formatting (`cargo fmt`)
- Use descriptive variable names
- Add documentation comments for public functions
- Handle errors appropriately with `Result<T, String>`

#### JSON Files (Snippets)
- Use descriptive keys and prefixes
- Include comprehensive descriptions
- Use tab stops (`${1:placeholder}`) for user input
- Group related snippets logically

### Commit Messages

Use conventional commit format:
```
type(scope): description

feat(commands): add bench console integration
fix(snippets): correct Python validation hook
docs(readme): update installation instructions
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

## 🎯 Contributing Areas

### 1. Slash Commands

**Adding new commands:**

1. Add to `slash_commands()` method in `src/lib.rs`:
```rust
SlashCommand {
    name: "frappe-new-command".to_string(),
    description: "Description of the command".to_string(),
    tooltip_text: "Detailed tooltip".to_string(),
    requires_argument: false,
}
```

2. Implement in `run_slash_command()`:
```rust
"frappe-new-command" => self.handle_new_command(args, worktree),
```

3. Create the handler method:
```rust
fn handle_new_command(&self, args: Vec<String>, worktree: &Worktree) -> Result<SlashCommandResult, String> {
    // Implementation
}
```

### 2. Code Snippets

**Adding Python snippets** (`snippets/python.json`):
```json
{
  "snippet_name": {
    "prefix": ["trigger", "alternate_trigger"],
    "body": [
      "def ${1:function_name}(${2:args}):",
      "\t\"\"\"${3:Description}\"\"\"",
      "\t${4:pass}"
    ],
    "description": "Clear description of what this snippet does"
  }
}
```

**Adding JavaScript snippets** (`snippets/javascript.json`):
```json
{
  "frappe_example": {
    "prefix": ["example", "frappe_example"],
    "body": [
      "frappe.ui.form.on('${1:DocType}', {",
      "\t${2:event}: function(frm) {",
      "\t\t${3:// Implementation}",
      "\t}",
      "});"
    ],
    "description": "Frappe form event handler"
  }
}
```

### 3. Language Support

**Enhancing language configurations:**

- Modify `languages/frappe-python/config.toml`
- Add new language directories for other file types
- Update Tree-sitter queries for better syntax highlighting

### 4. Templates and Generators

**Improving code generation:**

1. Update template methods in `src/lib.rs`
2. Add field type detection and suggestions
3. Enhance DocType, Page, and Report generators

## 🧪 Testing

### Manual Testing

1. **Install as dev extension** in Zed
2. **Test in real Frappe project**:
   - Create test bench environment
   - Try all slash commands
   - Test snippet expansions
   - Verify project detection

3. **Test edge cases**:
   - Non-Frappe projects
   - Missing dependencies
   - Invalid inputs

### Automated Testing

```bash
# Check Rust code
cargo check
cargo fmt --check
cargo clippy

# Validate JSON files
python -m json.tool snippets/python.json
python -m json.tool snippets/javascript.json
```

## 📋 Pull Request Process

### Before Submitting

- [ ] Test the extension thoroughly
- [ ] Update documentation if needed
- [ ] Add/update snippets with examples
- [ ] Follow code style guidelines
- [ ] Write clear commit messages

### PR Description Template

```markdown
## Description
Brief description of changes

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Documentation update
- [ ] Performance improvement

## Testing
- [ ] Tested as dev extension
- [ ] Tested in Frappe project
- [ ] All snippets work correctly
- [ ] No breaking changes

## Screenshots/Examples
If applicable, add screenshots or code examples
```

### Review Process

1. **Automated checks** must pass
2. **Code review** by maintainers
3. **Testing** in development environment
4. **Documentation** review if applicable

## 🐛 Bug Reports

### Before Reporting

1. **Search existing issues**
2. **Test with latest version**
3. **Reproduce in clean environment**

### Bug Report Template

```markdown
**Describe the bug**
Clear description of the issue

**To Reproduce**
Steps to reproduce:
1. Go to...
2. Click on...
3. See error

**Expected behavior**
What you expected to happen

**Environment:**
- OS: [e.g. macOS, Linux]
- Zed version: [e.g. 0.192.0]
- Frappe version: [e.g. v14, v15]
- Extension version: [e.g. 0.1.0]

**Additional context**
- Error logs from Zed
- Screenshots if applicable
- Related configuration
```

## 💡 Feature Requests

### Feature Request Template

```markdown
**Feature Description**
Clear description of the proposed feature

**Use Case**
Why is this feature needed? What problem does it solve?

**Proposed Implementation**
How should this feature work?

**Alternatives Considered**
Other solutions you've considered

**Additional Context**
Screenshots, examples, related issues
```

## 📚 Resources

### Frappe Development
- [Frappe Framework Documentation](https://frappeframework.com/docs)
- [Frappe Developer Guide](https://frappeframework.com/docs/v14/user/en/tutorial)
- [ERPNext Developer Guide](https://docs.erpnext.com/docs/v14/user/manual/en/setting-up)

### Zed Extension Development
- [Zed Extension API](https://docs.rs/zed_extension_api/latest/zed_extension_api/)
- [Zed Extension Documentation](https://zed.dev/docs/extensions)
- [Example Extensions](https://github.com/zed-industries/extensions)

### Rust Resources
- [The Rust Book](https://doc.rust-lang.org/book/)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
- [Rustlings](https://github.com/rust-lang/rustlings)

## 🤝 Community

- **GitHub Discussions**: For general questions and ideas
- **Issues**: For bug reports and feature requests
- **Frappe Forum**: For Frappe-specific discussions

## 📞 Getting Help

1. **Check documentation** first
2. **Search existing issues**
3. **Ask in discussions** for general help
4. **Create issue** for bugs or feature requests

---

Thank you for contributing to Latte! Your help makes this extension better for the entire Frappe developer community. 🙏