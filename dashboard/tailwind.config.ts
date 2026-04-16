import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        // Brand palette — pastel yellow
        brand: {
          50:  "#fffef0",
          100: "#fefce8",
          200: "#fef9c3",
          300: "#fef08a",
          400: "#ffe566",   // primary — bright pastel yellow
          500: "#f5c518",   // golden amber for hover
          600: "#d4a017",
          700: "#a37c12",
          800: "#6b500c",
          900: "#3d2e07",
        },
        surface: {
          50:  "#f8fafc",
          100: "#f1f5f9",
          200: "#e2e8f0",
          800: "#1e293b",
          900: "#0f172a",
          950: "#020617",
        },
      },
      fontFamily: {
        mono: ["'JetBrains Mono'", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      backgroundImage: {
        "grid-pattern": "radial-gradient(circle, #1e293b 1px, transparent 1px)",
        "hero-gradient": "radial-gradient(ellipse 80% 50% at 50% -20%, rgba(255,229,102,0.12), transparent)",
      },
      animation: {
        "pulse-slow": "pulse 4s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "fade-up": "fadeUp 0.5s ease-out forwards",
        "glow": "glow 2s ease-in-out infinite alternate",
      },
      keyframes: {
        fadeUp: {
          "0%": { opacity: "0", transform: "translateY(20px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        glow: {
          "0%": { boxShadow: "0 0 5px rgba(255,229,102,0.3)" },
          "100%": { boxShadow: "0 0 20px rgba(255,229,102,0.7), 0 0 40px rgba(255,229,102,0.3)" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
