# Tag Spider RS

A Rust-based web scraper for automated content extraction and management from Neos CMS systems. Features intelligent tree navigation, URL validation, and TF-IDF search capabilities.

## Features

- **Automated CMS Navigation**: Recursively traverse and interact with Neos CMS tree structures
- **Content Extraction**: Extract embedded content (external links, YouTube videos, metadata)
- **Tag Management**: Bulk add/clear tags on question-answer content
- **URL Validation**: Concurrent validation of extracted URLs with status reporting
- **Session Management**: Automatic re-login dialog detection and handling
- **TF-IDF Search**: Built-in document indexing and search with term frequency analysis
- **CSV Export**: Export extracted content with breadcrumb paths and metadata
- **Headless Mode**: Run browser automation in headless mode via environment variable

## Prerequisites

- Rust 1.70+ (2021 edition)
- Firefox browser
- [GeckoDriver](https://github.com/mozilla/geckodriver/releases) (must be running on `localhost:4444`)
- Nix (optional, for development environment)

## Installation

```bash
# Clone the repository
git clone <repository-url>
cd tag-spider-rs

# Build the project
cargo build --release

# Start GeckoDriver (in a separate terminal)
geckodriver --port 4444
```

## Configuration

### Credentials
Create a `credentials.json` file in the project root or at `/run/secrets/cms-pswd`:

```json
{
  "username": "your-username",
  "password": "your-password"
}
```

### File Tree
Place your CMS tree structure in `resources/tree.json` for navigation.

### Tags (Optional)
For tag management features, create `resources/tags.csv`:

```csv
question_id,tags
Q001,tag1,tag2,tag3
Q002,tag4,tag5
```

## Usage

### Basic Usage
```bash
# Run in normal (visible) mode
cargo run

# Run in headless mode
HEADLESS=true cargo run
```

### Interactive Commands
Once running, use these keyboard shortcuts:

- **`q`** - Quit the program
- **`a`** - Add tags (requires question-answer environment)
- **`c`** - Clear tags (requires question-answer environment)
- **`d`** - Bulk extract and validate dynamic content from folders

### Bulk Extraction Workflow
1. Press `d` to start bulk extraction
2. Enter the target folder's treeitem ID (e.g., `treeitem-c6643bf0-label`)
3. Choose whether to validate URLs (concurrent validation at the end)
4. Results are saved to `./embedded_content/{folder-id}.csv`

### Output Format
Extracted content includes:
- Source Node ID
- Breadcrumb Path
- Content Type (ExternalLink, YouTube)
- URL
- Title, Author, File Type, Size
- URL Validation Status

## Project Structure

```
src/
├── main.rs         # CLI interface and bulk extraction logic
├── spider.rs       # WebDriver automation and tree navigation
├── tree.rs         # File tree data structure
├── filenode.rs     # Tree node implementation
├── model.rs        # TF-IDF search model
├── lexer.rs        # Text tokenization and stemming
└── lib.rs          # Library exports
```

## Performance Optimizations

- Concurrent URL validation (configurable concurrency)
- Dynamic sleep times based on page load indicators
- Reduced polling delays for faster traversal
- Smart re-login detection before operations

## Development

```bash
# Run with Nix development shell
nix develop

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

## Notes

- Automatic session recovery handles timeout dialogs during long-running extractions
- Maximum traversal depth is configurable (default: 5 levels)
- URL validation uses a 10-second timeout per request

## License

See LICENSE file for details.
