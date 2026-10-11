import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const browserGlobals = {
  document: "readonly",
  HTMLCanvasElement: "readonly",
  KeyboardEvent: "readonly",
  Map: "readonly",
  Math: "readonly",
  Number: "readonly",
  ResizeObserver: "readonly",
  Set: "readonly",
  String: "readonly",
  window: "readonly"
};

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**", "src/generated/**"]
  },
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked.map((config) => ({
    ...config,
    files: ["**/*.{ts,tsx}"]
  })),
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        projectService: {
          allowDefaultProject: ["*.config.ts"]
        },
        tsconfigRootDir: import.meta.dirname
      },
      globals: browserGlobals
    },
    plugins: {
      "react-hooks": reactHooks
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "no-undef": "off",
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-unused-vars": ["error", { ignoreRestSiblings: true }],
      "@typescript-eslint/no-unnecessary-condition": "error",
      "@typescript-eslint/no-unsafe-argument": "error",
      "@typescript-eslint/no-unsafe-assignment": "error",
      "@typescript-eslint/no-unsafe-call": "error",
      "@typescript-eslint/no-unsafe-member-access": "error",
      "@typescript-eslint/no-unsafe-return": "error",
      "@typescript-eslint/restrict-template-expressions": [
        "error",
        {
          allowBoolean: true,
          allowNever: false,
          allowNullish: false,
          allowNumber: true,
          allowRegExp: false
        }
      ],
      "@typescript-eslint/strict-boolean-expressions": "error",
      "@typescript-eslint/switch-exhaustiveness-check": "error",
      eqeqeq: ["error", "always"],
      "no-debugger": "error",
      // Browser dialogs look like a web page; confirmations use the app's dialog.
      "no-alert": "error"
    }
  },
  {
    files: ["src/bindings.ts"],
    rules: {
      "@typescript-eslint/no-unnecessary-type-parameters": "off"
    }
  },
  {
    files: ["**/*.d.ts"],
    rules: {
      "@typescript-eslint/no-unnecessary-type-parameters": "off"
    }
  }
);
