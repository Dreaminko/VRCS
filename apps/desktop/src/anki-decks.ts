export type AnkiDeckNode = {
  name: string;
  label: string;
  depth: number;
  selectable: boolean;
  children: AnkiDeckNode[];
};

export type VisibleAnkiDeckNode = AnkiDeckNode & {
  expanded: boolean;
  hasChildren: boolean;
};

type MutableDeckNode = {
  name: string;
  label: string;
  selectable: boolean;
  children: Map<string, MutableDeckNode>;
};

const deckCollator = new Intl.Collator("zh-CN", {
  numeric: true,
  sensitivity: "base",
});

function toDeckNodes(
  nodes: Iterable<MutableDeckNode>,
  depth: number,
): AnkiDeckNode[] {
  return Array.from(nodes)
    .sort((left, right) => deckCollator.compare(left.label, right.label))
    .map((node) => ({
      name: node.name,
      label: node.label,
      depth,
      selectable: node.selectable,
      children: toDeckNodes(node.children.values(), depth + 1),
    }));
}

export function buildAnkiDeckTree(deckNames: readonly string[]): AnkiDeckNode[] {
  const roots = new Map<string, MutableDeckNode>();

  for (const deckName of new Set(deckNames.filter(Boolean))) {
    const segments = deckName.split("::").filter(Boolean);
    if (!segments.length) continue;

    let siblings = roots;
    const path: string[] = [];
    for (const segment of segments) {
      path.push(segment);
      const fullName = path.join("::");
      let node = siblings.get(segment);
      if (!node) {
        node = {
          name: fullName,
          label: segment,
          selectable: false,
          children: new Map(),
        };
        siblings.set(segment, node);
      }
      if (fullName === deckName) node.selectable = true;
      siblings = node.children;
    }
  }

  return toDeckNodes(roots.values(), 1);
}

export function ankiDeckAncestors(deckName: string): string[] {
  const segments = deckName.split("::").filter(Boolean);
  return segments.slice(0, -1).map((_, index) => segments.slice(0, index + 1).join("::"));
}

export function ankiDeckParent(deckName: string): string | null {
  const segments = deckName.split("::").filter(Boolean);
  return segments.length > 1 ? segments.slice(0, -1).join("::") : null;
}

export function ankiDeckDisplayName(deckName: string): string {
  return deckName.split("::").filter(Boolean).join(" / ");
}

export function visibleAnkiDeckNodes(
  nodes: readonly AnkiDeckNode[],
  expandedNames: ReadonlySet<string>,
): VisibleAnkiDeckNode[] {
  const visible: VisibleAnkiDeckNode[] = [];

  const visit = (items: readonly AnkiDeckNode[]) => {
    for (const node of items) {
      const hasChildren = node.children.length > 0;
      const expanded = hasChildren && expandedNames.has(node.name);
      visible.push({ ...node, hasChildren, expanded });
      if (expanded) visit(node.children);
    }
  };

  visit(nodes);
  return visible;
}
