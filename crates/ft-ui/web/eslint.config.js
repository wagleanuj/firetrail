import js from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import prettier from 'eslint-config-prettier'

export default tseslint.config(
  // Generated / vendored / build output — never lint these.
  {
    ignores: [
      'dist/',
      'node_modules/',
      'src/routeTree.gen.ts',
      // ts-rs / generated bindings output, if/when it appears.
      'src/lib/bindings/',
    ],
  },

  // Base JS + TypeScript recommended (syntax-only, no type-checking) rules.
  js.configs.recommended,
  ...tseslint.configs.recommended,

  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2022,
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      // Classic Rules of Hooks correctness checks — keep these as errors.
      ...reactHooks.configs.recommended.rules,

      // react-hooks v7 ships the experimental React Compiler diagnostics in its
      // recommended set. These are perf/optimization advisories, not Rules-of-
      // Hooks correctness violations: they flag legitimate patterns (accessing
      // refs in render for the force-graph viewer, syncing state in effects,
      // react-hook-form's un-memoizable `watch()`). We are not adopting the
      // React Compiler here, so downgrade them to warnings rather than churn
      // working code. `rules-of-hooks` and `exhaustive-deps` stay enforced.
      'react-hooks/set-state-in-effect': 'warn',
      'react-hooks/refs': 'warn',
      'react-hooks/incompatible-library': 'warn',

      // react-refresh: warn-only; HMR boundary hygiene, not a correctness gate.
      // shadcn/ui components commonly co-export variants (e.g. buttonVariants),
      // which is harmless but trips this rule.
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],

      // Allow intentionally-unused identifiers prefixed with `_`, and ignore
      // unused rest siblings (used for object-omit patterns). Downgraded to
      // warn so it doesn't fail the gate over harmless leftovers.
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          ignoreRestSiblings: true,
        },
      ],
    },
  },

  // Test files run in jsdom with vitest globals.
  {
    files: ['tests/**', '**/*.test.{ts,tsx}', '**/*.spec.{ts,tsx}'],
    languageOptions: {
      globals: {
        ...globals.node,
        ...globals.browser,
      },
    },
  },

  // Disable stylistic rules that conflict with Prettier (formatting is owned
  // by Prettier, not ESLint).
  prettier,
)
