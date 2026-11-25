import { Component, memo, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { openReleasePage } from "./app-updater";

const markdownComponents: Components = {
  a: ({ node: _node, href, children, ...props }) => {
    if (typeof href === "string" && /^https:\/\//i.test(href)) {
      return (
        <a
          {...props}
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(event) => {
            event.preventDefault();
            void openReleasePage(href);
          }}
        >
          {children}
        </a>
      );
    }
    return <>{children}</>;
  },
  img: ({ alt }) => <>{alt}</>,
};

class UpdateMarkdownBoundary extends Component<
  { source: string; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? (
      <pre className="update-notes-raw">{this.props.source}</pre>
    ) : (
      this.props.children
    );
  }
}

export const UpdateNotesMarkdown = memo(function UpdateNotesMarkdown({
  children,
}: {
  children: string;
}) {
  return (
    <div className="update-notes-markdown">
      <UpdateMarkdownBoundary key={children} source={children}>
        <Markdown remarkPlugins={[remarkGfm]} components={markdownComponents} skipHtml>
          {children}
        </Markdown>
      </UpdateMarkdownBoundary>
    </div>
  );
});
