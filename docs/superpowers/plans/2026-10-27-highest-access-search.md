# Highest access search implementation plan

Goal: Include web search and deferred tool discovery in the Run commands profile, including Discord runs.

- Add regression coverage for profile selection and discovery from explicit tool grants.
- Include the search and discovery controls in the highest profile; preserve individual permissions and other profiles.
- Enable the run-local discovery catalog when both discovery controls are explicitly granted, retaining existing settings and approval checks.
- Run focused web tests, required web checks, and the Rust suites; verify the running agent separately.
