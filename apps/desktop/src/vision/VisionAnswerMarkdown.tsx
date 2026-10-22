import { Children, Component, isValidElement, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import "./vision-answer-markdown.css";

function elementChildren(value: ReactNode): ReactNode {
  return isValidElement<{ children?: ReactNode }>(value) ? value.props.children : null;
}

function tableRows(children: ReactNode): ReactNode[][] {
  return Children.toArray(children).flatMap((child) => {
    if (!isValidElement<{ children?: ReactNode }>(child)) return [];
    if (child.type === "tr") return [Children.toArray(child.props.children).filter((cell) => isValidElement(cell))];
    return tableRows(child.props.children);
  });
}

function VerticalTable({ children }: { children?: ReactNode }) {
  const rows = tableRows(children);
  const first = rows[0] ?? [];
  const hasHeader = first.length > 0 && first.every((cell) => isValidElement(cell) && cell.type === "th");
  const labels = hasHeader ? first.map(elementChildren) : [];
  const body = hasHeader && rows.length > 1 ? rows.slice(1) : rows;
  return <div className="vision-answer-table-cards" role="table">
    {body.map((row, index) => <div className="vision-answer-table-row" role="row" key={index}>
      {row.map((cell, column) => <div className="vision-answer-table-cell" role="cell" key={column}>
        {hasHeader && rows.length > 1 && labels[column] && <strong className="vision-answer-table-label">{labels[column]}</strong>}
        <div>{elementChildren(cell)}</div>
      </div>)}
    </div>)}
  </div>;
}

const desktopComponents: Components = {
  a: ({ node: _node, ...props }) => props.href
    ? <a {...props} target={props.href.startsWith("#") ? undefined : "_blank"} rel="noopener noreferrer" />
    : <>{props.children}</>,
  img: ({ alt }) => <>{alt}</>,
  table: ({ node: _node, ...props }) => <div className="vision-answer-table vision-window-answer-table"><table {...props} /></div>,
};

const vrComponents: Components = {
  ...desktopComponents,
  a: ({ children }) => <>{children}</>,
  table: ({ children }) => <VerticalTable>{children}</VerticalTable>,
};

class MarkdownBoundary extends Component<{ source: string; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() { return { failed: true }; }

  render() {
    return this.state.failed ? <p className="vision-answer-raw">{this.props.source}</p> : this.props.children;
  }
}

export function VisionAnswerMarkdown({ children, vr = false }: { children: string; vr?: boolean }) {
  return <div className={`vision-answer-markdown${vr ? " vision-answer-markdown-vr" : ""}`}>
    <MarkdownBoundary key={children} source={children}>
      <Markdown remarkPlugins={[remarkGfm]} components={vr ? vrComponents : desktopComponents} skipHtml>{children}</Markdown>
    </MarkdownBoundary>
  </div>;
}
