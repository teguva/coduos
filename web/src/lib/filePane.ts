export type FileEntry = {
  name: string;
  path: string;
  dir: boolean;
  size: number;
  modified?: number | null;
};

export type FilePane = {
  id: string;
  root: string;
  path: string;
  list: {
    root: string;
    path: string;
    roots: { id: string; label: string; path: string }[];
    entries: FileEntry[];
    space?: { used: number; total: number } | null;
  } | null;
  query: string;
  selected: Set<string>;
  lastClicked: string | null;
  error: string;
};
