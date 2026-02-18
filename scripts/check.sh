#!/bin/bash
set -e

VERBOSE=0
if [ "$1" = "-v" ] || [ "$1" = "--verbose" ]; then
    VERBOSE=1
fi

echo "================================"
echo "x2ssh Project Health Check"
echo "================================"
if [ $VERBOSE -eq 1 ]; then
    echo "(Verbose mode - showing full output)"
fi
echo ""

FAILED=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

run_check() {
    local name="$1"
    local cmd="$2"
    local fix_cmd="$3"

    echo -n "Checking $name... "
    if [ $VERBOSE -eq 1 ]; then
        echo ""
        echo "  Command: $cmd"
        echo "  ---"
        if eval "$cmd"; then
            echo "  ---"
            echo -e "${GREEN}✓ $name passed${NC}"
            echo ""
            return 0
        else
            echo "  ---"
            echo -e "${RED}✗ $name failed${NC}"
            if [ -n "$fix_cmd" ]; then
                echo "  Fix: $fix_cmd"
            fi
            echo ""
            return 1
        fi
    else
        if eval "$cmd" > /dev/null 2>&1; then
            echo -e "${GREEN}✓${NC}"
            return 0
        else
            echo -e "${RED}✗${NC}"
            if [ -n "$fix_cmd" ]; then
                echo "  Fix: $fix_cmd"
            fi
            return 1
        fi
    fi
}

echo "=== Rust Checks ==="
echo ""

# Rust formatting
if ! run_check "Rust formatting" "cargo fmt -- --check" "cargo fmt"; then
    FAILED=1
fi

# Rust linting
if ! run_check "Rust linting (clippy)" "cargo clippy -- -D warnings"; then
    FAILED=1
fi

# Rust unit tests
if ! run_check "Rust unit tests" "cargo test"; then
    FAILED=1
fi

echo ""
echo "=== Python Checks ==="
echo ""

# Python formatting
if ! run_check "Python formatting" "uv run ruff format --check" "uv run ruff format"; then
    FAILED=1
fi

# Python linting
if ! run_check "Python linting" "uv run ruff check"; then
    FAILED=1
fi

# Python type checking
if ! run_check "Python type checking" "uv run ty check"; then
    FAILED=1
fi

echo ""
echo "=== E2E Tests ==="
echo ""

# E2E tests (requires Docker)
if ! run_check "E2E tests" "uv run pytest"; then
    FAILED=1
fi

echo ""
echo "================================"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All checks passed!${NC}"
    echo "================================"
    exit 0
else
    echo -e "${RED}Some checks failed!${NC}"
    echo "================================"
    echo ""
    echo "Run with -v or --verbose for detailed output"
    exit 1
fi
