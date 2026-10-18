import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const browser = [process.env.PROGRAMFILES, process.env["PROGRAMFILES(X86)"], process.env.LOCALAPPDATA]
  .filter(Boolean).flatMap(root => [
    join(root, "Google/Chrome/Application/chrome.exe"),
    join(root, "Microsoft/Edge/Application/msedge.exe"),
  ]).find(existsSync);

const scenarios = [
  { width: 1200, height: 800, scale: 100, locale: "zh-CN", theme: "light", view: "study", capture: "controls" },
  { width: 860, height: 620, scale: 100, locale: "en-US", theme: "dark", view: "card", capture: "controls" },
  { width: 1200, height: 800, scale: 150, locale: "zh-CN", theme: "dark", view: "study", capture: "segments" },
  { width: 860, height: 620, scale: 150, locale: "en-US", theme: "light", view: "card", capture: "examples" },
];

for (const scenario of scenarios) {
  const name = `${scenario.width}x${scenario.height}-${scenario.scale}-${scenario.theme}`;
  test(`learning workspace scrolls independently and retains edits at ${name}`, {
    timeout: 45000, skip: !browser && "No installed Chrome or Edge executable was found.",
  }, async () => {
    const bundle = await build({
      stdin: { resolveDir: join(testDirectory, ".."), loader: "tsx", contents: `
        import React, { useRef, useState } from 'react';
        import { createRoot } from 'react-dom/client';
        import i18next from 'i18next';
        import { initReactI18next } from 'react-i18next';
        import en from './src/i18n/locales/en-US.json';
        import zh from './src/i18n/locales/zh-CN.json';
        import { LearningWorkspace } from './src/learning/components/LearningWorkspace';
        import { BottomDock } from './src/shell/BottomDock';
        import { updateLearningItemEdits } from './src/learning/learning-edits';
        import { applyInterfaceScale } from './src/app/interface-scale';
        import { updateReducedMotionPreference } from './src/app/app-motion';
        import './src/styles.css';
        await i18next.use(initReactI18next).init({ lng: '${scenario.locale}', resources: {
          'en-US': { translation: en.translation }, 'zh-CN': { translation: zh.translation },
        } });
        document.documentElement.lang = '${scenario.locale}';
        document.documentElement.dataset.theme = '${scenario.theme}';
        await applyInterfaceScale(${scenario.scale});
        // Let focus callbacks run while Chromium advances its virtual clock.
        window.requestAnimationFrame = callback => setTimeout(() => callback(performance.now()), 16);
        window.cancelAnimationFrame = id => clearTimeout(id);
        const t = key => i18next.t(key);
        const noop = () => {};
        const analysis = { task_type: 'sentence_analysis', summary: '相手を誘うときの、やわらかい表現です。',
          current_meaning: '要不要一起去喝杯茶？', reading: 'いっしょにおちゃでものみませんか', base_form: '飲む',
          part_of_speech: '動詞', register: '丁寧', segments: [{ text: '飲みませんか', role: '誘い', explanation: '丁寧な誘いの表現。' }],
          grammar_points: [{ form: 'でも', meaning: 'など', note: '例を示す表現。' }], uncertainties: ['相手との関係は文脈から判断します。'],
          memory_tip: '否定疑問の形で、相手に選択の余地を残します。', examples: [{ source: '映画でも見ませんか。', translation: '要不要看场电影？' }],
          confidence: 'high', provider: 'Local service', model: 'study-model', prompt_version: 'v1' };
        const initialItems = Array.from({ length: 80 }, (_, index) => ({
          id: index + 1, kind: index === 1 ? 'word' : index === 2 ? 'excerpt' : 'sentence', status: index === 0 ? 'card_draft' : 'collected',
          source_text: '一緒にお茶でも飲みませんか。', working_text: '一緒にお茶でも飲みませんか。',
          selected_text: index === 1 ? 'お茶' : null, source_translation: index === 2 ? null : '要不要一起去喝杯茶？', source_language: 'ja', source_subtitle_ids: [],
          dictionary_entries: index === 2 ? [] : [{ term: 'お茶', reading: 'おちゃ', definition: '茶；茶水', dictionary: 'Sample dictionary' },
            { term: 'お茶', reading: 'おちゃ', definition: 'Tea', dictionary: 'Another dictionary' }],
          analysis: index === 0 ? analysis : null, analysis_stale: false, draft_stale: index === 0,
          draft: index === 0 ? { card_type: 'sentence', term: '一緒にお茶でも飲みませんか。', definition: '要不要一起去喝杯茶？', context: '休憩時間の会話', reading: '', language: 'ja', dictionary: '' } : null,
          anki_note_id: null, anki_sync_pending: false, created_at: '2026-10-18T08:00:00Z', updated_at: '2026-10-18T08:00:00Z',
        }));
        const subtitles = initialItems.slice(0, 30).map(item => ({ id: item.id, text: item.source_text,
          language: 'ja', source: 'speaker', created_at: item.created_at, translations: [{ text: item.source_translation }] }));
        window.calls = [];
        window.aiRequests = [];
        function App() {
          const [items, setItems] = useState(initialItems);
          const itemsRef = useRef(items);
          const [itemEdits, setEdits] = useState({});
          const [selectedId, setSelectedId] = useState(null);
          const [statusFilter, setStatusFilter] = useState('all');
          const [aiConfigured, setAiConfigured] = useState(false);
          const [busyByItem, setBusyByItem] = useState({});
          window.configureAi = () => setAiConfigured(true);
          const patch = (id, values) => {
            const next = { ...itemsRef.current.find(item => item.id === id), ...values };
            itemsRef.current = itemsRef.current.map(item => item.id === id ? next : item);
            setItems(itemsRef.current);
            return next;
          };
          const workspace = { items: statusFilter === 'all' ? items : items.filter(item => item.status === statusFilter),
            selectedId, setSelectedId, statusFilter, setStatusFilter, itemEdits,
            editItem: (id, values) => setEdits(current => updateLearningItemEdits(current, id, values)),
            loading: false, loadingMore: false, hasMore: true, loadMore: async () => {}, error: '', itemErrors: {}, busyByItem,
            locale: '${scenario.locale}', isItemBusy: id => Boolean(busyByItem[id]), isCollecting: () => false, isCaptured: () => false,
            preferences: { profileId: aiConfigured ? 'local' : '', model: aiConfigured ? 'study-model' : '' },
            updateWorkingText: async (id, working_text) => {
              window.calls.push('text'); return patch(id, { working_text, draft_stale: true, analysis_stale: true });
            },
            saveDraft: async (id, draft) => {
              window.calls.push('save'); return patch(id, { draft, draft_stale: false, status: 'card_draft' });
            },
            generateDraft: async (id, card_type) => {
              const item = itemsRef.current.find(item => item.id === id);
              window.calls.push('generate'); return patch(id, { draft: { ...initialItems[0].draft, card_type,
                term: item.working_text, definition: item.source_translation ?? item.dictionary_entries[0]?.definition ?? '' }, draft_stale: false });
            },
            exportItem: async id => {
              window.calls.push('export'); return patch(id, { anki_note_id: 123, anki_sync_pending: false, status: 'exported' });
            },
            collectSubtitle: async () => { setSelectedId(1); return itemsRef.current[0]; },
            collectSubtitles: async () => { setSelectedId(1); return itemsRef.current[0]; },
            analyze: async (id, task_type, focus) => {
              window.calls.push('analysis');
              window.aiRequests.push({ id, task_type, focus, working_text: itemsRef.current.find(item => item.id === id).working_text });
              setBusyByItem({ [id]: 'analysis' });
              await new Promise(resolve => { window.finishAnalysis = resolve; });
              const updated = patch(id, { analysis: { ...analysis, task_type }, analysis_stale: false });
              setBusyByItem({});
              return updated;
            }, archiveItem: noop, restoreItem: noop, deleteItem: noop,
          };
          return <div className="app-shell"><div className="window-chrome" /><div className="app-body">
            <div className="app-scroll-region"><main className="workspace workspace-learning">
              <LearningWorkspace workspace={workspace} ankiEnabled ankiAvailable conversation={{ title: 'Japanese conversation', startedAt: initialItems[0].created_at }}
                onOpenAiSettings={() => { window.settingsOpened = true; }}
                subtitles={subtitles} onSelect={async () => {}} hasOlder loading={false} loadingOlder={false} onLoadOlder={async () => {}} />
            </main></div></div>
            <BottomDock learningAvailable chatboxAvailable page="learning" running={false} captureDisabled={false} chatboxOpen={false} chatboxDisabled={false}
              chatboxButtonRef={null} onPageChange={noop} onCompact={noop} onChatbox={noop} onCapture={noop} onVision={noop} />
          </div>;
        }
        createRoot(document.getElementById('root')).render(<App />);
        const settle = () => new Promise(resolve => setTimeout(resolve, 40));
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const rect = element => element.getBoundingClientRect();
        const visible = element => Boolean(element && rect(element).width && rect(element).height);
        const button = key => [...document.querySelectorAll('button')].find(element => visible(element) && element.textContent.trim() === t(key));
        const field = key => [...document.querySelectorAll('label.learning-field')].find(element => element.querySelector('span')?.textContent === t(key))?.querySelector('input,textarea');
        const click = async element => { check(Boolean(element), 'Expected control exists'); element.click(); await settle(); };
        const input = async (element, value) => {
          Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), 'value').set.call(element, value);
          element.dispatchEvent(new Event('input', { bubbles: true })); await settle();
        };
        try {
          await settle(); await settle();
          const outer = document.querySelector('.app-scroll-region');
          const list = document.querySelector('.learning-item-list');
          const detailPane = document.querySelector('.learning-detail-pane');
          const detail = () => detailPane.querySelector('.learning-detail-scroll');
          const generationMotion = () => detailPane.querySelector('.learning-card-section').getAnimations()
            .find(animation => !(animation instanceof CSSAnimation) && !(animation instanceof CSSTransition) && animation.playState !== 'finished');
          const inbox = document.querySelector('.learning-inbox');
          const dock = document.querySelector('.bottom-dock');
          const compact = !visible(detailPane);
          check(Math.abs(rect(inbox).bottom - innerHeight) < 40 * ${scenario.scale / 100}, 'Inbox fills the window below the heading');
          check(rect(inbox).bottom > rect(dock).top, 'Bottom capsule floats above the workspace');
          check(outer.scrollHeight <= outer.clientHeight + 1, 'The page does not grow with the inbox');
          check(list.scrollHeight > list.clientHeight, 'Long inbox has its own scrolling list');
          list.scrollTop = 240; await settle();
          check(list.scrollTop > 200 && outer.scrollTop === 0, 'Scrolling the inbox does not scroll the page');
          await click(list.querySelector('button'));
          check(visible(detailPane), 'Selecting an item opens its details');
          check(visible(document.querySelector('.learning-back-button')) === compact, 'The back button is only shown in the compact layout');
          if (compact) check(!visible(list) && document.activeElement === detailPane, 'Compact details replace the queue and receive keyboard focus');
          check(Math.abs(rect(detail()).top - rect(document.querySelector('.learning-detail-header')).bottom) <= 1, 'The detail scroll area starts below the fixed title and navigation');
          check(!document.querySelector('.learning-copy-section textarea'), 'Working text starts in reading mode');
          check(![...document.querySelectorAll('.learning-copy-section summary')].some(element => element.textContent.includes(t('learning.detail.viewSource'))), 'Unchanged original is not repeated');
          check(document.querySelector('.learning-analysis > :first-child').textContent.includes(t('learning.analysis.currentMeaning')), 'Meaning in context appears before the analysis summary');
          const analysisResult = document.querySelector('.learning-analysis');
          for (const key of ['segments', 'grammarPoints', 'examples']) {
            const section = [...analysisResult.querySelectorAll('section')].find(element => element.querySelector('h4')?.textContent === t('learning.analysis.' + key));
            check(visible(section) && !section.closest('details:not([open])'), key + ' can be read without opening a disclosure');
          }
          check(analysisResult.querySelector('.learning-analysis-examples li').children.length === 2, 'An example and its translation occupy separate lines');
          check(rect(document.querySelector('.learning-focus-actions')).bottom < rect(analysisResult).top, 'Follow-up actions appear before the long analysis result');
          const detailTabs = document.querySelector('.learning-detail-navigation .learning-tabs');
          getComputedStyle(detailTabs, '::before').transform;
          await click(button('learning.detail.card'));
          const selectionMotion = detailTabs.getAnimations({ subtree: true })
            .find(animation => animation instanceof CSSTransition && animation.transitionProperty === 'transform');
          check(Boolean(selectionMotion), 'The selected tab slides between study and card');
          selectionMotion.pause();
          selectionMotion.currentTime = Number(selectionMotion.effect.getTiming().duration) / 4;
          const indicator = getComputedStyle(detailTabs, '::before');
          const slideOffset = new DOMMatrixReadOnly(indicator.transform).m41;
          check(slideOffset > 0 && slideOffset < parseFloat(indicator.width), 'The tab indicator renders an intermediate position');
          selectionMotion.finish();
          check(!button('learning.card.save').disabled && button('learning.card.export').disabled, 'A stale draft can be reviewed and saved without changing a field');
          await click(button('learning.detail.study'));
          await click(button('learning.detail.edit'));
          await input(document.querySelector('.learning-copy-section textarea'), 'Edited text kept across views');
          check(document.querySelector('.learning-copy-section .learning-disclosure summary')?.textContent.includes(t('learning.detail.viewSource')), 'Corrections reveal the collected original for comparison');
          await click(button('learning.detail.card'));
          let exportButton = button('learning.card.export');
          check(exportButton.disabled, 'Stale drafts cannot be exported');
          await input(field('learning.card.definition'), 'Edited definition');
          await click(button('learning.card.back'));
          check(document.querySelector('.learning-card-preview-definition').textContent === 'Edited definition', 'Preview follows the draft fields');
          await click(button('learning.detail.study'));
          check(document.querySelector('.learning-copy-section textarea').value === 'Edited text kept across views', 'Changing views preserves working text');
          detail().scrollTop = 160;
          // Virtual time coalesces native scroll events until after the test steps.
          detail().dispatchEvent(new Event('scroll'));
          await settle();
          const studyScroll = detail().scrollTop;
          if (compact) await click(button('learning.inbox.title'));
          check(list.scrollTop > 200, 'Returning to the queue preserves list position');
          await click(list.querySelectorAll('button')[1]);
          if (compact) await click(button('learning.inbox.title'));
          await click(list.querySelector('button'));
          check(document.querySelector('.learning-copy-section textarea').value === 'Edited text kept across views', 'Changing items preserves text edits');
          check(Math.abs(detail().scrollTop - studyScroll) <= 1, 'Changing items restores the detail scroll position: ' + detail().scrollTop + ' vs ' + studyScroll);
          await click(button('learning.detail.card'));
          check(field('learning.card.definition').value === 'Edited definition', 'Changing items preserves card edits');
          await click(button('learning.detail.study'));
          await click(button('learning.card.openDraft'));
          check(field('learning.card.definition').value === 'Edited definition' && window.calls.length === 0, 'Opening an existing card preserves unsaved edits without regeneration');
          await click(button('learning.card.save'));
          check(window.calls.join(',') === 'text,save', 'Saving a reviewed draft saves changed working text first');
          exportButton = button('learning.card.export');
          check(!exportButton.disabled, 'Saved draft is ready for export');
          await click(exportButton);
          check(button('learning.card.exported')?.disabled, 'Exported card shows its saved export state');
          check(!document.querySelector('.learning-workspace-header'), 'The workspace renders without the summary header');
          await click(button('learning.materials.title'));
          const materials = document.querySelector('.learning-material-surface');
          const search = materials.querySelector('input[type=search]');
          await input(search, '一緒');
          materials.scrollTop = 200; await settle();
          await click(button('learning.inbox.back'));
          await click(button('learning.materials.collect'));
          check(search.value === '一緒' && materials.scrollTop >= 190, 'Source filters and scroll survive returning to the inbox');
          await click(materials.querySelector('.learning-add-action'));
          await click(button('learning.materials.openCollected'));
          check(visible(detailPane), 'Collection feedback opens the collected item');
          if (compact) await click(button('learning.inbox.title'));
          await click(list.querySelectorAll('button')[1]);
          await click(button('learning.detail.study'));
          check(document.querySelector('.learning-word-reading').textContent === 'おちゃ', 'A word shows its reading next to the title');
          check(document.querySelector('.learning-copy-section > :first-child').textContent.includes('茶；茶水'), 'The main dictionary definition appears before the context');
          check(document.querySelector('.learning-dictionary-meaning').textContent.includes('Sample dictionary'), 'The main definition identifies its source');
          check(document.querySelector('.learning-reading-text mark').textContent === 'お茶', 'The target word is highlighted in context');
          check(!document.querySelector('.learning-dictionary-snapshot').open, 'Additional dictionary definitions start collapsed');
          check(document.querySelector('.learning-task-actions .dropdown-trigger').textContent.includes(t('learning.ai.tasks.contextual_word_explanation')) && button('learning.ai.analyze')?.disabled, 'The word analysis type is directly visible and analysis requires AI configuration');
          await click(button('learning.ai.openSettings'));
          check(window.settingsOpened, 'Missing AI configuration provides a learning settings entry');
          await click(button('learning.detail.edit'));
          await input(document.querySelector('.learning-copy-section textarea'), 'お茶を飲みませんか。');
          check(!button('learning.card.saveAndCreate').disabled, 'A card can be created from study without AI configuration');
          const beforeCreate = window.calls.length;
          await click(button('learning.card.saveAndCreate'));
          check(window.calls.slice(beforeCreate).join(',') === 'text,generate', 'Creating a card saves corrected text before generating');
          check(button('learning.detail.card').getAttribute('aria-pressed') === 'true', 'Creating from study opens the card view');
          check(Boolean(document.querySelector('.learning-card-preview')), 'Generation produces an editable card preview');
          const pop = generationMotion();
          check(Boolean(pop), 'Creating from study animates the generated card');
          const card = document.querySelector('.learning-card-section');
          pop.pause();
          pop.currentTime = Number(pop.effect.getTiming().duration) * 0.1;
          check(new DOMMatrixReadOnly(getComputedStyle(card).transform).a < 1 && Number(getComputedStyle(card).opacity) < 1, 'A generated card begins with a smaller, fading surface');
          pop.currentTime = Number(pop.effect.getTiming().duration) * 0.7;
          check(new DOMMatrixReadOnly(getComputedStyle(card).transform).a > 1, 'The card slightly overshoots before settling');
          pop.finish();
          await click(button('learning.card.back'));
          await click(button('learning.card.regenerate'));
          const regeneratedPop = generationMotion();
          check(Boolean(regeneratedPop) && regeneratedPop !== pop, 'Regeneration replays the entrance motion');
          check(Boolean(document.querySelector('.learning-card-preview-definition')), 'Regeneration keeps the selected preview side');
          regeneratedPop.finish();
          await input(field('learning.card.definition'), 'A refined definition');
          await click(button('learning.card.save'));
          check(!generationMotion(), 'Editing and saving a card do not replay its entrance');
          updateReducedMotionPreference(true);
          await click(button('learning.card.regenerate'));
          check(!generationMotion(), 'Reduced motion skips the card pop');
          check(parseFloat(getComputedStyle(document.querySelector('.learning-detail-navigation .learning-tabs'), '::before').transitionDuration) < 0.001, 'Reduced motion also settles the tab indicator');
          updateReducedMotionPreference(false);
          if (compact) await click(button('learning.inbox.title'));
          await click(list.querySelectorAll('button')[2]);
          await click(button('learning.detail.study'));
          check(document.querySelector('.learning-task-actions .dropdown-trigger').textContent.includes(t('learning.ai.tasks.session_review')) && button('learning.ai.analyze')?.disabled, 'An excerpt offers a conversation review');
          await click(button('learning.detail.card'));
          await click(button('learning.card.generate'));
          check(Boolean(generationMotion()), 'Generating inside the card view also animates the card');
          check(field('learning.card.definition').value === '' && button('learning.card.export').disabled, 'Missing meaning remains empty and blocks export');
          await input(field('learning.card.definition'), 'A translation supplied by the learner');
          check(!button('learning.card.save').disabled, 'An incomplete card can be completed in the editor');
          await click(button('learning.card.save'));
          check(!button('learning.card.export').disabled, 'A completed and saved card is ready to export');
          if (compact) await click(button('learning.inbox.title'));
          await click(list.querySelector('button'));
          await click(button('learning.detail.${scenario.view}'));
          detail().scrollTop = 0; await settle();
          check(outer.scrollHeight <= outer.clientHeight + 1 && outer.scrollTop === 0, 'Editing and switching views do not extend the page');
          check(document.documentElement.scrollWidth <= innerWidth + 1, 'Workspace does not overflow horizontally');
          const headerTop = rect(document.querySelector('.learning-detail-header')).top;
          detail().scrollTop = detail().scrollHeight;
          await settle();
          check(Math.abs(rect(document.querySelector('.learning-detail-header')).top - headerTop) <= 1, 'Detail navigation stays fixed while its content scrolls');
          check(Math.abs(rect(detail()).top - rect(document.querySelector('.learning-detail-header')).bottom) <= 1, 'The scrollbar remains confined to the work area');
          check(rect(document.querySelector('.learning-detail-view:not([hidden])').lastElementChild).bottom < rect(dock).top, 'The final detail content scrolls clear of the capsule');
          if (compact) await click(button('learning.inbox.title'));
          list.scrollTop = list.scrollHeight;
          await settle();
          check(rect(list.querySelector('.learning-inbox-pagination')).bottom < rect(dock).top, 'The final queue action scrolls clear of the capsule');
          list.scrollTop = 0;
          await settle();
          if (compact) await click(list.querySelector('button'));
          await click(button('learning.detail.study'));
          window.configureAi(); await settle();
          const chooseTask = async task => {
            const trigger = document.querySelector('.learning-task-actions .dropdown-trigger');
            trigger.scrollIntoView({ block: 'center' }); await settle();
            await click(trigger);
            const option = [...document.querySelectorAll('[role=option]')].find(element => visible(element) && element.textContent.trim() === t('learning.ai.tasks.' + task));
            await click(option);
          };
          const beforeModeChange = window.calls.length;
          await chooseTask('contextual_word_explanation');
          check(window.calls.length === beforeModeChange && visible(document.querySelector('.learning-analysis')), 'Choosing an analysis type keeps the current result and does not send a request');
          check(!button('learning.ai.analyze').querySelector('svg'), 'The analysis action uses a plain text label before loading');
          if (!document.querySelector('.learning-copy-section textarea')) await click(button('learning.detail.edit'));
          const correctedText = '一緒に温かいお茶でも飲みませんか。';
          await input(document.querySelector('.learning-copy-section textarea'), correctedText);
          const beforeAnalysis = window.calls.length;
          await click(button('learning.ai.analyze'));
          check(window.calls.slice(beforeAnalysis).join(',') === 'text,analysis', 'Analysis saves corrected text before sending the request');
          check(window.aiRequests.at(-1).working_text === correctedText && window.aiRequests.at(-1).task_type === 'contextual_word_explanation', 'The request uses the selected task and corrected text');
          check(button('learning.ai.analyzing')?.disabled && document.querySelector('.learning-task-actions .dropdown-trigger').disabled && [...document.querySelectorAll('.learning-focus-actions button')].every(element => element.disabled), 'Analysis controls prevent duplicate requests while loading');
          check(document.querySelector('.learning-ai-section [role=status]').textContent === t('learning.ai.analyzing') && visible(document.querySelector('.learning-analysis')), 'Analysis reports progress while retaining the current result');
          window.finishAnalysis(); await settle();
          check(!button('learning.ai.reanalyze')?.disabled && !document.querySelector('.learning-ai-section [role=status]'), 'Completed analysis restores the action and clears stale feedback');
          await chooseTask('sentence_analysis');
          await click(button('learning.ai.focus.examples'));
          check(window.aiRequests.at(-1).task_type === 'contextual_word_explanation' && window.aiRequests.at(-1).focus === 'examples', 'Follow-up actions use the displayed analysis type');
          window.finishAnalysis(); await settle();
          await click(button('learning.detail.saveWorkingText'));
          const captureTarget = document.querySelector('${scenario.capture === "segments" ? ".learning-analysis-segments" : scenario.capture === "examples" ? ".learning-analysis-examples" : ".learning-ai-section"}');
          detail().scrollTop += (rect(captureTarget).top - rect(detail()).top) / ${scenario.scale / 100} - 12;
          await settle();
          check(detail().scrollWidth <= detail().clientWidth + 1 && document.documentElement.scrollWidth <= innerWidth + 1, 'Expanded analysis remains inside the work area at this width and scale');
          document.getElementById('result').textContent = 'PASS';
        } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.message; }
      ` },
      bundle: true, write: false, outfile: 'learning-workspace-test.js', format: 'esm', platform: 'browser', jsx: 'automatic',
      define: { 'import.meta.env.VITE_VRCS_SESSION_TOKEN': '""' },
    });
    const directory = mkdtempSync(join(testDirectory, '.learning-workspace-browser-'));
    try {
      const javascript = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
      const stylesheet = bundle.outputFiles.find(file => file.path.endsWith('.css')).text;
      const errorCapture = '<script>const result = document.getElementById("result"); window.addEventListener("error", event => result.textContent = "FAIL: " + event.message); window.addEventListener("unhandledrejection", event => result.textContent = "FAIL: " + String(event.reason)); new MutationObserver(() => parent.postMessage(result.textContent, "*")).observe(result, { childList: true });</script>';
      writeFileSync(join(directory, 'content.html'), '<meta charset="utf-8"><style>' + stylesheet + '</style><div id="root"></div><pre id="result" hidden>PENDING</pre>'
        + errorCapture + '<script type="module">' + javascript.replace(/<\/script/gi, '<\\/script') + '</script>');
      writeFileSync(join(directory, 'test.html'), '<style>body{margin:0}</style><iframe src="content.html" style="display:block;border:0;width:' + scenario.width + 'px;height:' + scenario.height + 'px"></iframe><pre id="result" hidden>PENDING</pre>'
        + '<script>window.addEventListener("message", event => { if (event.source === document.querySelector("iframe").contentWindow) document.getElementById("result").textContent = event.data; });</script>');
      const args = ['--headless=new', '--dump-dom', '--virtual-time-budget=10000', '--disable-gpu', '--no-first-run',
        '--disable-background-networking', '--disable-extensions', '--force-device-scale-factor=1',
        '--window-size=' + scenario.width + ',' + (scenario.height + 200), '--user-data-dir=' + join(directory, 'profile')];
      if (process.env.VRCS_LEARNING_SCREENSHOTS) {
        mkdirSync(process.env.VRCS_LEARNING_SCREENSHOTS, { recursive: true });
        args.push('--screenshot=' + join(process.env.VRCS_LEARNING_SCREENSHOTS, name + '.png'));
      }
      const result = spawnSync(browser, [...args, pathToFileURL(join(directory, 'test.html')).href], {
        encoding: 'utf8', timeout: 30000, maxBuffer: 8 * 1024 * 1024,
      });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      const report = result.stdout.match(/<pre id="result" hidden="">([^<]*)<\/pre>/)?.[1];
      assert.equal(report, 'PASS', report || result.stderr);
    } finally {
      assert.ok(directory.startsWith(join(testDirectory, '.learning-workspace-browser-')));
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}
