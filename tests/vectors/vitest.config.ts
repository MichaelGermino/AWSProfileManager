import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['tests/vectors/**/*.test.ts'],
    environment: 'node',
    // Record mode rewrites the vector files at the end of each file's suite.
    sequence: { concurrent: false },
  },
});
