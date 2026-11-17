const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('ui/app.js', 'utf8');

function createUiContext(moduleIndex, extra = {}) {
  const context = vm.createContext({
    currentPayload: { moduleIndex },
    activeCanvas: moduleIndex.canvases?.[0] || null,
    activeTabPage: null,
    overlayTabPage: new Map(),
    showNullCanvas: false,
    layoutEditMode: false,
    previewEnabled: false,
    selectedObject: null,
    queryRecordsByBlock: new Map(),
    activeRecordIndexByBlock: new Map(),
    runtime: { active: false },
    formsSvg: {
      attrs: {},
      innerHTML: '',
      setAttribute(k, v) { this.attrs[k] = v; },
      querySelectorAll() { return []; },
    },
    canvasStats: { textContent: '' },
    oracleWindowTitle: null,
    oracleStatusMsg: null,
    applyZoom() {},
    unescapeXml: value => String(value ?? ''),
    escapeHtml: value => String(value ?? '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;'),
    ...extra,
  });

  vm.runInContext(
    source.slice(source.indexOf('  const ORACLE_NAMED_COLORS ='), source.indexOf('  // Initialize')),
    context
  );
  vm.runInContext(
    source.slice(source.indexOf('  function canvasCoordinateScale('), source.indexOf('  // Visual Canvas 2D SVG Rendering')),
    context
  );
  vm.runInContext(
    source.slice(source.indexOf('  function buildCanvasLayer('), source.indexOf('  window.selectSvgGraphic =')),
    context
  );
  return context;
}

test('normalizeOracleColor and normalizeOracleFontSize handle Oracle palettes and 1/100pt sizes', () => {
  const ctx = createUiContext({ blocks: [], canvases: [], graphics: [] });
  assert.equal(vm.runInContext('normalizeOracleColor("r100g100b100")', ctx), '#ffffff');
  assert.equal(vm.runInContext('normalizeOracleColor("R50G0B100")', ctx), '#8000ff');
  assert.equal(vm.runInContext('normalizeOracleColor("gray4")', ctx), '#f5f5f5');
  assert.equal(vm.runInContext('normalizeOracleColor("gray50")', ctx), '#808080');
  assert.equal(vm.runInContext('normalizeOracleColor("darkblue")', ctx), '#000080');
  assert.equal(vm.runInContext('normalizeOracleColor("olafcanvas")', ctx), '#d4d0c8');
  assert.equal(vm.runInContext('normalizeOracleColor("automatic")', ctx), null);
  assert.equal(vm.runInContext('normalizeOracleFontSize(900)', ctx), 11);
  assert.equal(vm.runInContext('normalizeOracleFontSize(1000)', ctx), 13);
  assert.equal(vm.runInContext('normalizeOracleFontSize(12)', ctx), 12);
});

test('graphics respect FillPattern=transparent/none, EdgeForegroundColor, Line, and Group', () => {
  const mod = {
    key: { name: 'DEPT' },
    canvases: [{ name: 'MAIN', width: 500, height: 300, tabPages: [] }],
    blocks: [],
    graphics: [
      {
        name: 'G_TRANSP',
        canvasName: 'MAIN',
        graphicsType: 'Rectangle',
        fillPattern: 'transparent',
        backColor: 'black',
        edgeForegroundColor: 'darkblue',
        lineWidth: 2,
        xPosition: 10,
        yPosition: 10,
        width: 120,
        height: 40,
      },
      {
        name: 'G_LINE',
        canvasName: 'MAIN',
        graphicsType: 'Line',
        edgeForegroundColor: 'gray50',
        xPosition: 10,
        yPosition: 60,
        width: 200,
        height: 0,
      },
      {
        name: 'G_GROUP',
        canvasName: 'MAIN',
        graphicsType: 'Group',
        backColor: 'black',
        xPosition: 10,
        yPosition: 10,
        width: 300,
        height: 200,
      },
    ],
  };
  const ctx = createUiContext(mod);
  const layer = vm.runInContext('buildCanvasLayer(currentPayload.moduleIndex.canvases[0], null)', ctx);
  assert.match(layer.svgHtml, /fill="none" stroke="#000080" stroke-width="2"/);
  assert.match(layer.svgHtml, /<line x1="10" y1="60" x2="210" y2="60"/);
  assert.doesNotMatch(layer.svgHtml, /fill="black"/);
});

test('multiline prompts/labels render tspans and single-instance buttons do not repeat in multi-row blocks', () => {
  const mod = {
    key: { name: 'EMP' },
    canvases: [{ name: 'MAIN', width: 600, height: 400, tabPages: [] }],
    graphics: [],
    blocks: [
      {
        name: 'EMP',
        recordsDisplayed: 4,
        distanceBetweenRecords: 4,
        items: [
          {
            name: 'EMPNO',
            itemType: 'Text Item',
            canvasName: 'MAIN',
            prompt: 'Emp\nNo',
            promptAttachmentEdge: 'Top',
            promptAttachmentOffset: 6,
            xPosition: 20,
            yPosition: 50,
            width: 80,
            height: 20,
            itemsDisplay: 4,
          },
          {
            name: 'BTN_COMMIT',
            itemType: 'Push Button',
            canvasName: 'MAIN',
            label: 'Save\nAll',
            xPosition: 200,
            yPosition: 50,
            width: 90,
            height: 30,
            itemsDisplay: 1,
          },
          {
            name: 'RG_TYPE',
            itemType: 'Radio Group',
            canvasName: 'MAIN',
            prompt: 'Type',
            xPosition: 20,
            yPosition: 180,
            width: 150,
            height: 55,
            itemsDisplay: 1,
            radioButtons: [
              { name: 'RB_FT', label: 'Full Time', xPosition: 28, yPosition: 192 },
              { name: 'RB_PT', label: 'Part Time', xPosition: 28, yPosition: 212 },
            ],
          },
        ],
      },
    ],
  };
  const ctx = createUiContext(mod);
  const layer = vm.runInContext('buildCanvasLayer(currentPayload.moduleIndex.canvases[0], null)', ctx);
  assert.match(layer.svgHtml, /<tspan[^>]*>Emp<\/tspan><tspan[^>]*>No<\/tspan>/);
  assert.match(layer.svgHtml, /<tspan[^>]*>Save<\/tspan><tspan[^>]*>All<\/tspan>/);
  assert.equal((layer.svgHtml.match(/data-item="EMPNO"/g) || []).length, 4);
  assert.equal((layer.svgHtml.match(/data-item="BTN_COMMIT"/g) || []).length, 1);
  assert.match(layer.svgHtml, /Full Time/);
  assert.match(layer.svgHtml, /Part Time/);
});

test('Tab and Stacked overlay canvases clip to viewport and filter tabPageName case-insensitively', () => {
  const mod = {
    key: { name: 'ORDERS' },
    canvases: [
      { name: 'MAIN', canvasType: 'Content', windowName: 'WIN1', width: 700, height: 500, tabPages: [] },
      {
        name: 'TAB_OV',
        canvasType: 'Tab',
        windowName: 'WIN1',
        width: 500,
        height: 300,
        viewportWidth: 420,
        viewportHeight: 220,
        viewportXPosition: 25,
        viewportYPosition: 80,
        tabPages: [
          { name: 'PG_ONE', label: 'General' },
          { name: 'PG_TWO', label: 'Details' },
        ],
      },
    ],
    graphics: [],
    blocks: [
      {
        name: 'ORDERS',
        recordsDisplayed: 1,
        items: [
          {
            name: 'ORDER_ID',
            itemType: 'Text Item',
            canvasName: 'TAB_OV',
            tabPageName: 'pg_one',
            xPosition: 20,
            yPosition: 45,
            width: 100,
            height: 20,
          },
          {
            name: 'DETAIL_NOTE',
            itemType: 'Text Item',
            canvasName: 'TAB_OV',
            tabPageName: 'PG_TWO',
            xPosition: 20,
            yPosition: 45,
            width: 100,
            height: 20,
          },
        ],
      },
    ],
  };
  const ctx = createUiContext(mod);
  vm.runInContext('renderCanvasSvg()', ctx);
  const svg = ctx.formsSvg.innerHTML;
  assert.match(svg, /clipPath id="ov-clip-TAB_OV"><rect x="0" y="0" width="420" height="220"/);
  assert.match(svg, /transform="translate\(25,80\)" clip-path="url\(#ov-clip-TAB_OV\)"/);
  assert.match(svg, /clipPath id="tab-body-clip-TAB_OV"/);
  assert.match(svg, /data-item="ORDER_ID"/);
  assert.doesNotMatch(svg, /data-item="DETAIL_NOTE"/);
});

test('TabPage content translates below tab strip and renders associated Stacked and nested Tab canvases per active tab', () => {
  const mod = {
    key: { name: 'DEPT' },
    coordinate: { coordinateSystem: 'Real', realUnit: 'Centimeter' },
    canvases: [
      {
        name: 'CV_TAB_MAIN',
        canvasType: 'Tab',
        windowName: 'WIN0',
        width: 20000,
        height: 11000,
        tabPages: [
          { name: 'SEARCH_TAB', label: 'Search', associatedCanvases: ['CV_SEARCH_HDR', 'CV_SEARCH_DEF'] },
          { name: 'SUMMARY_TAB', label: 'Summary', associatedCanvases: [] },
          { name: 'RESULTS_TAB', label: 'Results', associatedCanvases: ['CV_NESTED_TABS'] },
        ],
      },
      {
        name: 'CV_SEARCH_HDR',
        windowName: 'WIN0',
        width: 18000,
        height: 3000,
        viewportWidth: 18000,
        viewportHeight: 3000,
        viewportXPosition: 200,
        viewportYPosition: 1000,
        tabPages: [],
      },
      {
        name: 'CV_SEARCH_DEF',
        canvasType: 'Stacked',
        windowName: 'WIN0',
        width: 18000,
        height: 2500,
        viewportWidth: 18000,
        viewportHeight: 2500,
        viewportXPosition: 200,
        viewportYPosition: 4500,
        tabPages: [],
      },
      {
        name: 'CV_NESTED_TABS',
        canvasType: 'Tab',
        windowName: 'WIN0',
        width: 18000,
        height: 9000,
        viewportWidth: 18000,
        viewportHeight: 9000,
        viewportXPosition: 200,
        viewportYPosition: 1000,
        tabPages: [
          { name: 'SUB_A', label: 'Sub A' },
          { name: 'SUB_B', label: 'Sub B' },
        ],
      },
    ],
    graphics: [],
    blocks: [
      {
        name: 'DEPT',
        recordsDisplayed: 1,
        items: [
          { name: 'SEARCH_KEY', canvasName: 'CV_SEARCH_HDR', itemType: 'Text Item', xPosition: 500, yPosition: 300, width: 3000, height: 500 },
          { name: 'DEF_FIELD', canvasName: 'CV_SEARCH_DEF', itemType: 'Text Item', xPosition: 500, yPosition: 300, width: 3000, height: 500 },
          {
            name: 'LOC_COL',
            canvasName: 'CV_TAB_MAIN',
            tabPageName: 'SUMMARY_TAB',
            itemType: 'Text Item',
            prompt: "Loc'n",
            promptAttachmentEdge: 'Top',
            promptAttachmentOffset: 343,
            xPosition: 500,
            yPosition: 900,
            width: 2500,
            height: 500,
          },
          { name: 'SUB_A_ITEM', canvasName: 'CV_NESTED_TABS', tabPageName: 'SUB_A', itemType: 'Text Item', xPosition: 400, yPosition: 800, width: 2500, height: 500 },
          { name: 'SUB_B_ITEM', canvasName: 'CV_NESTED_TABS', tabPageName: 'SUB_B', itemType: 'Text Item', xPosition: 400, yPosition: 800, width: 2500, height: 500 },
        ],
      },
    ],
  };

  const ctx = createUiContext(mod);
  ctx.activeTabPage = 'SEARCH_TAB';
  vm.runInContext('renderCanvasSvg()', ctx);
  assert.match(ctx.formsSvg.innerHTML, /data-item="SEARCH_KEY"/);
  assert.match(ctx.formsSvg.innerHTML, /data-item="DEF_FIELD"/);
  assert.doesNotMatch(ctx.formsSvg.innerHTML, /data-item="SUB_A_ITEM"/);

  ctx.activeTabPage = 'SUMMARY_TAB';
  vm.runInContext('renderCanvasSvg()', ctx);
  assert.match(ctx.formsSvg.innerHTML, /transform="translate\(0,26\)"/);
  assert.match(ctx.formsSvg.innerHTML, /data-item="LOC_COL"/);
  assert.doesNotMatch(ctx.formsSvg.innerHTML, /data-item="SEARCH_KEY"/);

  ctx.activeTabPage = 'RESULTS_TAB';
  vm.runInContext('renderCanvasSvg()', ctx);
  assert.match(ctx.formsSvg.innerHTML, /data-item="SUB_A_ITEM"/);
  assert.doesNotMatch(ctx.formsSvg.innerHTML, /data-item="SUB_B_ITEM"/);

  ctx.overlayTabPage.set('CV_NESTED_TABS', 'SUB_B');
  vm.runInContext('renderCanvasSvg()', ctx);
  assert.match(ctx.formsSvg.innerHTML, /data-item="SUB_B_ITEM"/);
  assert.doesNotMatch(ctx.formsSvg.innerHTML, /data-item="SUB_A_ITEM"/);
});

test('EWT bevels, PromptAlign/PromptAttachmentOffset, RadioGroup buttons, and Frame title masking render accurately', () => {
  const mod = {
    key: { name: 'ORDERS' },
    coordinate: { coordinateSystem: 'Real', realUnit: 'Point' },
    canvases: [
      {
        name: 'CV_TAB',
        canvasType: 'Tab',
        windowName: 'WIN0',
        width: 500,
        height: 300,
        tabPages: [{ name: 'PG_MAIN', label: 'Main', backColor: 'r95g95b92' }],
      },
    ],
    graphics: [
      {
        name: 'FRM_SEARCH',
        canvasName: 'CV_TAB',
        tabPageName: 'PG_MAIN',
        graphicsType: 'Frame',
        frameTitle: 'Search By',
        foreColor: 'white',
        fillPattern: 'transparent',
        xPosition: 10,
        yPosition: 10,
        width: 240,
        height: 90,
      },
    ],
    blocks: [
      {
        name: 'ORDERS',
        recordsDisplayed: 1,
        items: [
          {
            name: 'ORDER_NO',
            canvasName: 'CV_TAB',
            tabPageName: 'PG_MAIN',
            itemType: 'Text Item',
            prompt: 'Order #:',
            promptAttachmentEdge: 'End',
            promptAttachmentOffset: 8,
            promptAlign: 'Center',
            promptAlignOffset: 2,
            bevel: 'Lowered',
            xPosition: 20,
            yPosition: 30,
            width: 80,
            height: 20,
          },
          {
            name: 'SEARCH_BY',
            canvasName: 'CV_TAB',
            tabPageName: 'PG_MAIN',
            itemType: 'Radio Group',
            xPosition: 200,
            yPosition: 30,
            width: 100,
            height: 50,
            radioButtons: [
              { name: 'RB_ID', label: 'Order ID', xPosition: 20, yPosition: 28, visible: true },
              { name: 'RB_CUST', label: 'Customer', xPosition: 20, yPosition: 48, visible: true },
              { name: 'RB_HIDDEN', label: 'Hidden', xPosition: 20, yPosition: 68, visible: false },
            ],
          },
        ],
      },
    ],
  };

  const ctx = createUiContext(mod);
  vm.runInContext('renderCanvasSvg()', ctx);
  const svg = ctx.formsSvg.innerHTML;

  // Frame title uses dark default instead of invisible white FillForegroundColor and masks with tabPage backColor
  assert.match(svg, /fill="#f2f2eb"/);
  assert.match(svg, /fill="#000000">Search By<\/text>/);

  // PromptAttachmentEdge=End + PromptAlign=Center places prompt to the right of item (x=20+80+8=108)
  assert.match(svg, /<text x="108" y="46" text-anchor="start"[^>]*>Order #:<\/text>/);

  // RadioGroup renders visible RadioButtons at their own canvas positions without an opaque rect box
  assert.match(svg, /cx="26" cy="35" r="6"/);
  assert.match(svg, />Order ID<\/text>/);
  assert.match(svg, />Customer<\/text>/);
  assert.doesNotMatch(svg, />Hidden<\/text>/);
});
