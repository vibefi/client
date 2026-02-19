import React, { useEffect, useRef } from "react";
import { Compartment, EditorState } from "@codemirror/state";
import { indentWithTab, defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching } from "@codemirror/language";
import { searchKeymap } from "@codemirror/search";
import { oneDark } from "@codemirror/theme-one-dark";
import { drawSelection, EditorView, keymap, lineNumbers } from "@codemirror/view";
import { languageExtensionFromPath, positionForLine } from "./utils";

export type CodeEditorProps = {
  filePath: string;
  value: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  jumpToLine?: number;
  jumpNonce?: number;
  onJumpHandled?: () => void;
  readOnly?: boolean;
};

export function CodeEditor({
  filePath,
  value,
  onChange,
  onBlur,
  jumpToLine,
  jumpNonce,
  onJumpHandled,
  readOnly = false,
}: CodeEditorProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const onBlurRef = useRef(onBlur);
  const readOnlyCompartmentRef = useRef(new Compartment());
  const languageCompartmentRef = useRef(new Compartment());

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    onBlurRef.current = onBlur;
  }, [onBlur]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const readOnlyCompartment = readOnlyCompartmentRef.current;
    const languageCompartment = languageCompartmentRef.current;
    const baseExtensions = [
      lineNumbers(),
      history(),
      drawSelection(),
      bracketMatching(),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
      oneDark,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        onChangeRef.current(update.state.doc.toString());
      }),
      EditorView.domEventHandlers({
        blur: () => {
          onBlurRef.current();
        },
      }),
      readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
      languageCompartment.of(languageExtensionFromPath(filePath)),
    ];

    const state = EditorState.create({
      doc: value,
      extensions: baseExtensions,
    });

    const view = new EditorView({
      state,
      parent: container,
    });
    view.contentDOM.spellcheck = false;
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;

    const effects = [];
    effects.push(
      readOnlyCompartmentRef.current.reconfigure(EditorState.readOnly.of(readOnly)),
      languageCompartmentRef.current.reconfigure(languageExtensionFromPath(filePath))
    );

    const currentDoc = view.state.doc.toString();
    if (currentDoc !== value) {
      view.dispatch({
        changes: { from: 0, to: currentDoc.length, insert: value },
        effects,
      });
      return;
    }

    view.dispatch({ effects });
  }, [filePath, readOnly, value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !jumpNonce || !jumpToLine || jumpToLine < 1) {
      return;
    }

    const content = view.state.doc.toString();
    const anchor = positionForLine(content, jumpToLine);
    view.dispatch({
      selection: { anchor },
      scrollIntoView: true,
    });
    view.focus();
    onJumpHandled?.();
  }, [jumpNonce, jumpToLine, onJumpHandled]);

  return <div className="editor-codemirror" ref={containerRef} />;
}
