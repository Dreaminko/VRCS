import { useEffect, useRef, useState } from "react";

import { prefersReducedMotion } from "../app/app-motion";
import {
  conversationSwitchDirection,
  type ConversationSwitchDirection,
} from "./conversation-state";

export const CONVERSATION_EXIT_MS = 85;
export const CONVERSATION_ENTER_MS = 160;
export const CONVERSATION_SKELETON_DELAY_MS = 200;

export type ConversationTransitionPhase =
  | "idle"
  | "exiting"
  | "waiting"
  | "skeleton"
  | "entering";

interface StageSnapshot<T> {
  key: string | null;
  id: string | null;
  data: T;
}

export function useConversationTransition<T>({
  targetKey,
  committedKey,
  targetId = null,
  committedId = null,
  orderedIds,
  loading,
  data,
}: {
  targetKey: string | null;
  committedKey: string | null;
  targetId?: string | null;
  committedId?: string | null;
  orderedIds?: readonly string[];
  loading: boolean;
  data: T;
}): {
  phase: ConversationTransitionPhase;
  direction: ConversationSwitchDirection;
  displayedKey: string | null;
  displayedData: T;
} {
  const [phase, setPhase] = useState<ConversationTransitionPhase>("idle");
  const [activeDirection, setActiveDirection] = useState<ConversationSwitchDirection>("neutral");
  const [snapshot, setSnapshot] = useState<StageSnapshot<T>>(() => ({
    key: committedKey,
    id: committedId,
    data,
  }));

  const activeDirectionRef = useRef<ConversationSwitchDirection>("neutral");
  const liveSnapshotRef = useRef<StageSnapshot<T>>({
    key: committedKey,
    id: committedId,
    data,
  });
  const latestRef = useRef({
    targetKey,
    committedKey,
    targetId,
    committedId,
    orderedIds,
    loading,
    data,
  });
  latestRef.current = {
    targetKey,
    committedKey,
    targetId,
    committedId,
    orderedIds,
    loading,
    data,
  };

  const exitTimerRef = useRef<number | null>(null);
  const enterTimerRef = useRef<number | null>(null);
  const skeletonTimerRef = useRef<number | null>(null);

  const isLive = snapshot.key === committedKey && (phase === "idle" || phase === "entering");
  if (isLive) {
    liveSnapshotRef.current = { key: committedKey, id: committedId, data };
  }
  const displayedData = isLive
    ? data
    : snapshot.key === liveSnapshotRef.current.key
      ? liveSnapshotRef.current.data
      : snapshot.data;

  useEffect(() => {
    const clearExitTimer = () => {
      if (exitTimerRef.current !== null) {
        window.clearTimeout(exitTimerRef.current);
        exitTimerRef.current = null;
      }
    };
    const clearEnterTimer = () => {
      if (enterTimerRef.current !== null) {
        window.clearTimeout(enterTimerRef.current);
        enterTimerRef.current = null;
      }
    };
    const clearSkeletonTimer = () => {
      if (skeletonTimerRef.current !== null) {
        window.clearTimeout(skeletonTimerRef.current);
        skeletonTimerRef.current = null;
      }
    };
    const clearAllTimers = () => {
      clearExitTimer();
      clearEnterTimer();
      clearSkeletonTimer();
    };

    const resolveDirection = (
      fromId: string | null,
      toId: string | null,
    ): ConversationSwitchDirection => {
      const ids = latestRef.current.orderedIds;
      return ids ? conversationSwitchDirection(ids, fromId, toId) : "neutral";
    };

    const setDirection = (nextDirection: ConversationSwitchDirection) => {
      activeDirectionRef.current = nextDirection;
      setActiveDirection(nextDirection);
    };

    const commit = (
      nextKey: string | null,
      nextId: string | null,
      nextData: T,
      reduced: boolean,
      nextDirection: ConversationSwitchDirection,
    ) => {
      clearAllTimers();
      setSnapshot({ key: nextKey, id: nextId, data: nextData });
      if (reduced || nextKey === null) {
        setDirection("neutral");
        setPhase("idle");
        return;
      }
      setDirection(nextDirection);
      setPhase("entering");
      enterTimerRef.current = window.setTimeout(() => {
        enterTimerRef.current = null;
        setDirection("neutral");
        setPhase("idle");
      }, CONVERSATION_ENTER_MS);
    };

    const scheduleSkeleton = () => {
      if (skeletonTimerRef.current !== null) return;
      skeletonTimerRef.current = window.setTimeout(() => {
        skeletonTimerRef.current = null;
        const latest = latestRef.current;
        const ready = latest.targetKey !== null
          && latest.committedKey === latest.targetKey
          && !latest.loading;
        if (ready) {
          commit(
            latest.committedKey,
            latest.committedId,
            latest.data,
            prefersReducedMotion(),
            activeDirectionRef.current,
          );
          return;
        }
        setPhase((current) => (
          current === "waiting" || (prefersReducedMotion() && current === "idle")
            ? "skeleton"
            : current
        ));
      }, CONVERSATION_SKELETON_DELAY_MS);
    };

    const reduced = prefersReducedMotion();
    const targetReady = targetKey !== null && committedKey === targetKey && !loading;

    if (targetKey === null) {
      clearAllTimers();
      if (snapshot.key !== null || snapshot.id !== null) {
        setSnapshot({ key: null, id: null, data: latestRef.current.data });
      }
      if (activeDirectionRef.current !== "neutral") setDirection("neutral");
      if (phase !== "idle") setPhase("idle");
      return;
    }

    if (targetKey === snapshot.key) {
      if (phase !== "idle" && phase !== "entering" && targetReady) {
        commit(committedKey, committedId, latestRef.current.data, reduced, "neutral");
      }
      return;
    }

    if (reduced) {
      clearExitTimer();
      clearEnterTimer();
      if (targetReady) {
        commit(committedKey, committedId, latestRef.current.data, true, "neutral");
      } else if (phase !== "skeleton") {
        scheduleSkeleton();
      }
      return;
    }

    const switchDirection = resolveDirection(snapshot.id, targetId);

    if (snapshot.key !== null) {
      if (phase === "idle" || phase === "entering") {
        clearEnterTimer();
        setDirection(switchDirection);
        setSnapshot(liveSnapshotRef.current);
        setPhase("exiting");
        clearSkeletonTimer();
        if (!targetReady) scheduleSkeleton();
        exitTimerRef.current = window.setTimeout(() => {
          exitTimerRef.current = null;
          const latest = latestRef.current;
          const ready = latest.targetKey !== null
            && latest.committedKey === latest.targetKey
            && !latest.loading;
          if (ready) {
            commit(
              latest.committedKey,
              latest.committedId,
              latest.data,
              prefersReducedMotion(),
              activeDirectionRef.current,
            );
          } else {
            setPhase((current) => (current === "exiting" ? "waiting" : current));
          }
        }, CONVERSATION_EXIT_MS);
      } else if (phase === "exiting") {
        setDirection(switchDirection);
        clearSkeletonTimer();
        if (!targetReady) scheduleSkeleton();
      } else if (targetReady) {
        commit(committedKey, committedId, latestRef.current.data, false, switchDirection);
      } else if (phase === "waiting") {
        scheduleSkeleton();
      }
      return;
    }

    if (targetReady) {
      commit(committedKey, committedId, latestRef.current.data, false, "neutral");
    } else if (phase === "idle") {
      setDirection("neutral");
      setPhase("waiting");
      scheduleSkeleton();
    }
  }, [committedId, committedKey, loading, phase, snapshot.id, snapshot.key, targetId, targetKey]);

  useEffect(() => () => {
    if (exitTimerRef.current !== null) window.clearTimeout(exitTimerRef.current);
    if (enterTimerRef.current !== null) window.clearTimeout(enterTimerRef.current);
    if (skeletonTimerRef.current !== null) window.clearTimeout(skeletonTimerRef.current);
  }, []);

  return {
    phase,
    direction: activeDirection,
    displayedKey: snapshot.key,
    displayedData,
  };
}
