## Description

<!-- Provide a clear and concise description of your changes -->

### Type of Change

<!-- Check all that apply -->

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Performance improvement
- [ ] Code refactoring
- [ ] CI/CD changes
- [ ] Dependency updates

## Related Issues

<!-- Link related issues below. Use keywords to auto-close issues when PR is merged. -->

Closes #
Relates to #

## Changes Made

<!-- List the main changes made in this PR -->

-
-
-

## Testing

<!-- Describe the testing you've done -->

### Test Coverage

- [ ] Added unit tests for new functionality
- [ ] Added integration tests where appropriate
- [ ] Updated existing tests to reflect changes
- [ ] All tests pass locally (`cargo nextest run --workspace`)
- [ ] Doc tests pass (`cargo test --doc --workspace`)
- [ ] Code coverage is adequate (70%+ overall, 80%+ for new code)

### Manual Testing

<!-- Describe any manual testing performed -->

-
-

## Code Quality

<!-- Check all that apply -->

- [ ] Code follows [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt)
- [ ] Code is formatted with nightly rustfmt (`cargo +nightly fmt --all`)
- [ ] Clippy passes with no warnings (`cargo clippy --all-targets --all-features --workspace -- -D warnings`)
- [ ] Documentation is complete and accurate (`cargo doc --no-deps --all-features --workspace`)
- [ ] Public APIs are well-documented with examples
- [ ] Error handling is appropriate (thiserror for libraries, anyhow only in CLI)
- [ ] Strong types used instead of primitives where appropriate
- [ ] No `unsafe` code (or justified and documented if absolutely necessary)

## Performance Impact

<!-- If applicable, describe any performance implications -->

- [ ] No performance impact
- [ ] Performance improved (provide benchmarks)
- [ ] Performance may be impacted (explain why and provide benchmarks)
- [ ] Ran benchmarks: `cargo bench --workspace`

## Breaking Changes

<!-- If this PR includes breaking changes, describe them here -->

**Does this PR introduce breaking changes?**

- [ ] No breaking changes
- [ ] Yes, breaking changes (describe below)

### Breaking Change Details

<!-- For breaking changes, provide:
1. What breaks
2. Why the change is necessary
3. How users should migrate their code
4. Example migration code if applicable
-->

## Documentation

<!-- Check all that apply -->

- [ ] README.md updated (if needed)
- [ ] CONTRIBUTING.md updated (if needed)
- [ ] CHANGELOG.md updated with user-facing changes
- [ ] API documentation updated (rustdoc comments)
- [ ] Examples updated or added
- [ ] Architecture Decision Record (ADR) created (if significant architectural change)

## Checklist

<!-- Ensure all items are checked before requesting review -->

- [ ] I have read and followed the [CONTRIBUTING.md](../CONTRIBUTING.md) guidelines
- [ ] My code follows the project's style guidelines and lints
- [ ] I have performed a self-review of my own code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] I have made corresponding changes to the documentation
- [ ] My changes generate no new warnings or errors
- [ ] I have added tests that prove my fix is effective or that my feature works
- [ ] New and existing unit tests pass locally with my changes
- [ ] Any dependent changes have been merged and published
- [ ] I have checked my code and corrected any misspellings
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) format

## Additional Context

<!-- Add any other context, screenshots, or information about the PR here -->

## Reviewer Notes

<!-- Optional: Add any specific areas you'd like reviewers to focus on -->

---

**For Maintainers:**

- [ ] CI checks passing
- [ ] Code review completed
- [ ] Documentation review completed
- [ ] Security implications reviewed (if applicable)
- [ ] Performance benchmarks reviewed (if applicable)
- [ ] Breaking changes documented in CHANGELOG.md (if applicable)
- [ ] Ready to merge
