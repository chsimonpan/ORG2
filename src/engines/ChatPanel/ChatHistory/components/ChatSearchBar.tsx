/**
 * ChatSearchBar Component
 *
 * "Find in chat" search bar reusing the shared SearchInput component
 * (same as TerminalSearchPanel) for visual consistency.
 *
 * Features:
 * - Case-sensitive, whole-word & regex toggle buttons
 * - Result count and up/down navigation
 * - Escape to close
 */
import { X } from "lucide-react";
import {
  type RefObject,
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { SearchInput } from "@src/components/SearchInput";

import type { UseChatSearchReturn } from "../hooks/useChatSearch";

// ============================================
// Types
// ============================================

export interface ChatSearchBarProps {
  /** Search state from useChatSearch hook */
  search: UseChatSearchReturn;
  /** Callback when search bar is closed */
  onClose?: () => void;
  /** Whether the search bar is visible */
  isVisible: boolean;
}

export interface ChatSearchBarHandle {
  /** Focus the search input */
  focus: () => void;
}

// ============================================
// Component
// ============================================

export const ChatSearchBar = forwardRef<
  ChatSearchBarHandle | null,
  ChatSearchBarProps
>(function ChatSearchBar({ search, onClose, isVisible }, ref) {
  const { t } = useTranslation("sessions");
  const inputRef = useRef<HTMLInputElement>(null);
  const [resultMode, setResultMode] = useState<"matches" | "turns">("matches");

  const {
    query,
    setQuery,
    isSearching,
    resultCount,
    results,
    currentResultIndex,
    navigateToResult,
    nextResult,
    prevResult,
    clearSearch,
    caseSensitive,
    toggleCaseSensitive,
    useRegex,
    toggleRegex,
    wholeWord,
    toggleWholeWord,
  } = search;

  const previewResults = useMemo(() => {
    const rows = results.map((result, resultIndex) => ({
      result,
      resultIndex,
      turnKey: result.turnKey,
    }));
    if (resultMode === "matches") return rows;
    const seen = new Set<string>();
    return rows.filter(({ turnKey }) => {
      if (seen.has(turnKey)) return false;
      seen.add(turnKey);
      return true;
    });
  }, [resultMode, results]);

  // Expose focus method
  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
  }));

  // Focus input when visible
  useEffect(() => {
    if (isVisible) {
      const timer = setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [isVisible]);

  // Handle Escape to close
  useEffect(() => {
    if (!isVisible) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        clearSearch();
        onClose?.();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isVisible, clearSearch, onClose]);

  // Handle close button
  const handleClose = useCallback(() => {
    clearSearch();
    onClose?.();
  }, [clearSearch, onClose]);

  // Handle Enter → next result
  const handleSubmit = useCallback(() => {
    nextResult();
  }, [nextResult]);

  if (!isVisible) return null;

  return (
    <div className="relative flex items-center gap-2 px-2 py-1.5">
      <SearchInput
        value={query}
        onChange={setQuery}
        placeholder={t("chat.findInChat")}
        variant="panel"
        caseSensitive={caseSensitive}
        wholeWord={wholeWord}
        useRegex={useRegex}
        onCaseSensitiveToggle={toggleCaseSensitive}
        onWholeWordToggle={toggleWholeWord}
        onRegexToggle={toggleRegex}
        onPrevious={resultCount > 0 ? prevResult : undefined}
        onNext={resultCount > 0 ? nextResult : undefined}
        onSubmit={handleSubmit}
        inputRef={inputRef as RefObject<HTMLInputElement>}
        hideChevron
        inputBoxClassName="flex-none w-full max-w-[240px]"
      />

      {/* Result count */}
      <span className="min-w-[56px] shrink-0 text-center text-xs text-text-3">
        {!query
          ? ""
          : isSearching
            ? "..."
            : resultCount > 0
              ? `${currentResultIndex + 1} / ${resultCount}`
              : t("chat.noResults")}
      </span>

      <div className="flex items-center rounded bg-fill-2 p-0.5 text-[11px]">
        <button
          type="button"
          className={`rounded px-2 py-0.5 ${resultMode === "matches" ? "bg-fill-1 text-text-1" : "text-text-3"}`}
          onClick={() => setResultMode("matches")}
        >
          逐条结果
        </button>
        <button
          type="button"
          className={`rounded px-2 py-0.5 ${resultMode === "turns" ? "bg-fill-1 text-text-1" : "text-text-3"}`}
          onClick={() => setResultMode("turns")}
        >
          按 Turn
        </button>
      </div>

      {/* Close button — pushed to the right end */}
      <div className="flex flex-1 justify-end">
        <button
          onClick={handleClose}
          className="flex h-5 w-5 items-center justify-center rounded text-text-3 transition-colors hover:bg-fill-3 hover:text-text-1"
          title={t("chat.closeEsc")}
        >
          <X size={14} />
        </button>
      </div>

      {query && previewResults.length > 0 && (
        <div className="absolute left-2 right-2 top-full z-50 mt-1 max-h-72 overflow-y-auto rounded-md border border-border-2 bg-bg-1 p-1 shadow-xl">
          {previewResults.map(({ result, resultIndex, turnKey }) => (
            <button
              key={`${turnKey}:${result.item.id}:${resultIndex}`}
              type="button"
              className={`block w-full rounded px-2 py-1.5 text-left hover:bg-fill-2 ${resultIndex === currentResultIndex ? "bg-fill-1" : ""}`}
              onClick={() => navigateToResult(resultIndex)}
            >
              <div className="mb-0.5 flex items-center justify-between gap-2 text-[10px] text-text-3">
                <span>{result.item.source === "user" ? "用户" : result.item.source === "assistant" ? "助手" : "系统"}</span>
                <span>{new Date(result.item.createdAt).toLocaleString()}</span>
              </div>
              <div className="line-clamp-3 whitespace-pre-wrap break-words text-xs text-text-1">
                {result.snippet || result.item.displayText}
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
});

export default ChatSearchBar;
