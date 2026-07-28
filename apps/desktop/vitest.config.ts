import { defineConfig } from 'vitest/config';

// Kept separate from vite.config.ts because the `test` key belongs to Vitest's own config type,
// and putting it in the Vite config makes the Vite types reject it.
export default defineConfig({
  test: {
    // Only pure logic is unit tested here. Component behaviour is covered by the Rust-side
    // contract tests and by manual QA, so no DOM environment is pulled in and the suite stays
    // fast enough to run on every save.
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
