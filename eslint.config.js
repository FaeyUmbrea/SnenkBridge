import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';

export default ts.config(
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs['flat/recommended'],
  { files: ['frontend/**/*.{ts,svelte}'], languageOptions: { globals: globals.browser } },
  { files: ['**/*.svelte'], languageOptions: { parserOptions: { parser: ts.parser } } },
);
