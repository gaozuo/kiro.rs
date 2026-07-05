Status: DONE
Commits created: feat: add oauth login dialog
Build summary: `pnpm --dir admin-ui build` passed; production bundle built successfully.
Concerns: Build still prints existing non-blocking Vite/Node deprecation warnings about `esbuild` and `module.register()`.
Report file path: /Volumes/990PRO-2T/VectorBound/Project-Code/coffee-project/kiro.rs/.worktrees/admin-oauth-login/.superpowers/sdd/task-6-report.md

Fix section:
- Changed files: admin-ui/src/components/oauth-login-dialog.tsx
- Build result: `pnpm --dir admin-ui build` exited 0 and produced a production build (`vite v8.0.11`, 1915 modules transformed, built in 448ms); warnings remained for deprecated `esbuild` option and Node `module.register()`.
