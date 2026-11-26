// Mantine, dressed as the Void: pale stone surfaces, slate text, one colour that acts (the
// gate blue), ember for errors only, and Atkinson Hyperlegible throughout.

import { createTheme, type MantineColorsTuple } from "@mantine/core";

// the gate blue (#3a3fb0) at index 6, lighter above, deeper below
const gate: MantineColorsTuple = [
  "#eef0fb",
  "#dadcf4",
  "#b3b6e8",
  "#898ddc",
  "#666bd2",
  "#5056cb",
  "#3a3fb0",
  "#30359a",
  "#2a2e8a",
  "#1f2370",
];

// ember (#b3402a): errors only
const ember: MantineColorsTuple = [
  "#fdf0ed",
  "#f6dcd5",
  "#ecb5a8",
  "#e18b78",
  "#d8684f",
  "#d25236",
  "#b3402a",
  "#9d3724",
  "#8a301f",
  "#742718",
];

// moon: the quiet greys and slate
const moon: MantineColorsTuple = [
  "#fbfcfe",
  "#e9edf4",
  "#c9d0de",
  "#a9b2c6",
  "#8b96ae",
  "#6f7c9a",
  "#5a6684",
  "#454f68",
  "#343b4f",
  "#262c3a",
];

export const theme = createTheme({
  primaryColor: "gate",
  primaryShade: 6,
  colors: { gate, ember, moon },
  black: "#262c3a",
  white: "#fbfcfe",
  fontFamily: '"Atkinson", system-ui, sans-serif',
  headings: { fontFamily: '"Atkinson", system-ui, sans-serif', fontWeight: "700" },
  defaultRadius: "md",
  radius: { md: "10px", lg: "18px" },
  cursorType: "pointer",
  components: {
    Paper: { defaultProps: { radius: "lg", p: "xl", withBorder: true, className: "vox-slab" } },
    Alert: { defaultProps: { variant: "light" } },
  },
});
