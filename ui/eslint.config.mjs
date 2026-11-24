// @ts-check
// Lint only; no formatter, on purpose: a formatter would reformat the whole
// tree in one commit. CI runs `bun run lint`, which fails on warnings too.
import js from '@eslint/js'
import { defineConfig } from 'eslint/config'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'
import globals from 'globals'

export default defineConfig(
  { ignores: ['dist/**'] },
  {
    files: ['src/**/*.{ts,tsx}'],
    extends: [js.configs.recommended, tseslint.configs.recommended],
    plugins: { 'react-hooks': reactHooks },
    languageOptions: { globals: globals.browser },
    linterOptions: { reportUnusedDisableDirectives: 'error' },
    rules: {
      // The two classic hook rules. ponytail: the plugin's React Compiler
      // rules (refs, purity, set-state-in-effect, immutability) stay off,
      // decided with the React 19 move (#38). The app doesn't use the compiler
      // (its hot paths are three.js and the engine, not React renders), and
      // the 9 findings they raise are deliberate patterns, not bugs: three.js's
      // imperative API (raycaster, OrbitControls), setState in effects,
      // useMenuAction's latest-handler ref. Revisit if the compiler is adopted.
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'error',
      // `_name` marks a parameter a callback signature requires but we don't use.
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    },
  },
)
