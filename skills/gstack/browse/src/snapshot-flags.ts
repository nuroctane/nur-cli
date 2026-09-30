export interface SnapshotOptions {
  interactive?: boolean;
  compact?: boolean;
  depth?: number;
  selector?: string;
  diff?: boolean;
  annotate?: boolean;
  outputPath?: string;
  cursorInteractive?: boolean;
  heatmap?: string;
}

export const SNAPSHOT_FLAGS: Array<{
  short: string;
  long: string;
  description: string;
  takesValue?: boolean;
  valueHint?: string;
  optionKey: keyof SnapshotOptions;
}> = [
  { short: '-i', long: '--interactive', description: 'Interactive elements only (buttons, links, inputs) with @e refs. Also auto-enables cursor-interactive scan (-C) to capture dropdowns and popovers.', optionKey: 'interactive' },
  { short: '-c', long: '--compact', description: 'Compact (no empty structural nodes)', optionKey: 'compact' },
  { short: '-d', long: '--depth', description: 'Limit tree depth (0 = root only, default: unlimited)', takesValue: true, valueHint: '<N>', optionKey: 'depth' },
  { short: '-s', long: '--selector', description: 'Scope to CSS selector', takesValue: true, valueHint: '<sel>', optionKey: 'selector' },
  { short: '-D', long: '--diff', description: 'Unified diff against previous snapshot (first call stores baseline)', optionKey: 'diff' },
  { short: '-a', long: '--annotate', description: 'Annotated screenshot with red overlay boxes and ref labels', optionKey: 'annotate' },
  { short: '-o', long: '--output', description: 'Output path for annotated screenshot (default: <temp>/browse-annotated.png)', takesValue: true, valueHint: '<path>', optionKey: 'outputPath' },
  { short: '-C', long: '--cursor-interactive', description: 'Cursor-interactive elements (@c refs — divs with pointer, onclick). Auto-enabled when -i is used.', optionKey: 'cursorInteractive' },
  { short: '-H', long: '--heatmap', description: 'Color-coded overlay screenshot from JSON map: \'{"@e1":"green","@e3":"red"}\'. Valid colors: green, yellow, red, blue, orange, gray.', takesValue: true, valueHint: '<json>', optionKey: 'heatmap' },
];
