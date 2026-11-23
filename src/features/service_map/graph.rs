//! Static map of the attraction-inc services and their main runtime connections.
//!
//! Source of truth: `attraction-inc/workspace`, `ARCHITECTURE.md` (runtime map, component
//! ownership) and `docker-compose.yml`. Update this file when the architecture changes.

pub const ORG: &str = "attraction-inc";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Surface,
    Service,
    Data,
    Package,
    External,
    Tooling,
}

pub struct Node {
    pub id: &'static str,
    pub name: &'static str,
    /// GitHub repositories (in the org) whose code is this node.
    pub repos: &'static [&'static str],
    pub kind: Kind,
    pub description: &'static str,
    /// Grid position: column (0..=6) and lane (row).
    pub col: f32,
    pub lane: usize,
}

pub struct Edge {
    pub from: &'static str,
    pub to: &'static str,
    pub label: &'static str,
}

pub const LANES: [&str; 5] = [
    "Clients",
    "Runtime services",
    "Specs & external systems",
    "Shared packages",
    "Infra & tooling",
];

pub const NODES: &[Node] = &[
    // Lane 0: user-facing surfaces
    Node { id: "frontend", name: "Studio Frontend", repos: &["studio-frontend"], kind: Kind::Surface, col: 2.0, lane: 0,
        description: "Jameo & Attraction Studio (same React app, tenant build) incl. the design editor and Speculoos" },
    Node { id: "website", name: "Attraction Website", repos: &["attraction-website"], kind: Kind::Surface, col: 5.0, lane: 0,
        description: "Public multilingual marketing & product-discovery site (static product pages from Fibra)" },
    // Lane 1: runtime services
    Node { id: "multiplayer", name: "Multiplayer", repos: &["multiplayer"], kind: Kind::Service, col: 0.0, lane: 1,
        description: "WebSocket presence and real-time collaboration (DynamoDB connection state)" },
    Node { id: "render-service", name: "Render Service", repos: &["render-service"], kind: Kind::Service, col: 1.0, lane: 1,
        description: "Stateless server-side WebGL rendering for artwork, previews and backend PDFs" },
    Node { id: "design-state", name: "Design State", repos: &["design-state", "uw-designer"], kind: Kind::Service, col: 2.0, lane: 1,
        description: "Design records, mutations, product definitions and preview images (DynamoDB/S3)" },
    Node { id: "asset-manager", name: "Asset Manager", repos: &["asset-manager"], kind: Kind::Service, col: 3.0, lane: 1,
        description: "Python/FastAPI: uploads, artwork authorization, image processing, stitch estimates" },
    Node { id: "backend", name: "Studio Backend", repos: &["studio-backend"], kind: Kind::Service, col: 4.0, lane: 1,
        description: "Users, teams, roles/permissions, orders, pricing, techpacks, sessions (Postgres)" },
    Node { id: "fibra", name: "Fibra · Public API", repos: &["fibra", "api"], kind: Kind::Service, col: 5.0, lane: 1,
        description: "Catalogue, inventory, public API (api.attraction.com), MCP, Prextra syncs, distribution" },
    // Lane 2: specs + external systems
    Node { id: "specs-service", name: "Specs Service", repos: &["studio-specs"], kind: Kind::Service, col: 3.5, lane: 2,
        description: "Specs revisions (Speculoos), runtime merge, publish of public/private specs.json" },
    Node { id: "specs-baseline", name: "Baseline Specs", repos: &["specs-as-a-file"], kind: Kind::Data, col: 2.0, lane: 2,
        description: "Legacy versioned baseline specs.json (products, decorations, pricing) in private S3" },
    Node { id: "prextra", name: "Prextra ERP", repos: &[], kind: Kind::External, col: 5.0, lane: 2,
        description: "Company ERP; system of record for inventory, posts product/inventory/pricing syncs" },
    Node { id: "distributors", name: "Distributors", repos: &[], kind: Kind::External, col: 6.2, lane: 2,
        description: "SAGE Connect CA/US, ASI/ESP+, PromoStandards consumers" },
    // Lane 3: shared packages
    Node { id: "webgl", name: "WebGL Decorations", repos: &["webgl-decorations"], kind: Kind::Package, col: 1.0, lane: 3,
        description: "@attraction-inc/webgl-decorations: the Generator (renders designs) for Studio & Render Service" },
    Node { id: "specs-validation", name: "Specs Validation", repos: &["specs-validation"], kind: Kind::Package, col: 2.4, lane: 3,
        description: "@attraction-inc/specs-validation: specs contracts shared by Frontend and Specs Service" },
    Node { id: "translations", name: "Studio Translations", repos: &["studio-translations"], kind: Kind::Package, col: 3.7, lane: 3,
        description: "@attraction-inc/studio-translations for Frontend, Backend, Fibra; validates specs labels" },
    Node { id: "pdf-maker", name: "PDF Maker", repos: &["typst_pdf_generator"], kind: Kind::Package, col: 5.0, lane: 3,
        description: "Typst techpacks, mockups and client proofs generated for Backend" },
    // Lane 4: infra & tooling
    Node { id: "infra", name: "AWS Infra", repos: &["attraction-infra", "attraction-infra-us-east-1"], kind: Kind::Tooling, col: 1.0, lane: 4,
        description: "Shared AWS infrastructure (ca-central-1 + us-east-1), internal Slack automations" },
    Node { id: "workspace", name: "Workspace", repos: &["workspace"], kind: Kind::Tooling, col: 2.75, lane: 4,
        description: "Local integration stack (docker-compose), architecture docs, scripts" },
    Node { id: "live-e2e", name: "Live E2E", repos: &["live-e2e"], kind: Kind::Tooling, col: 4.5, lane: 4,
        description: "Playwright smoke tests against deployed Studios (staging & prod)" },
];

pub const EDGES: &[Edge] = &[
    // Studio frontend
    Edge { from: "frontend", to: "backend", label: "API, session" },
    Edge { from: "frontend", to: "design-state", label: "designs, previews" },
    Edge { from: "frontend", to: "asset-manager", label: "uploads" },
    Edge { from: "frontend", to: "render-service", label: "artwork previews" },
    Edge { from: "frontend", to: "multiplayer", label: "WebSocket" },
    Edge { from: "frontend", to: "specs-service", label: "Speculoos revisions" },
    Edge { from: "frontend", to: "fibra", label: "catalogue" },
    // Website
    Edge { from: "website", to: "fibra", label: "products, inventory, pricing" },
    // Backend
    Edge { from: "backend", to: "design-state", label: "designs for orders/techpacks" },
    Edge { from: "backend", to: "asset-manager", label: "stitch estimates, assets" },
    Edge { from: "backend", to: "render-service", label: "PDF renders" },
    Edge { from: "backend", to: "specs-service", label: "private catalogue" },
    Edge { from: "backend", to: "fibra", label: "catalogue order, minimums" },
    Edge { from: "backend", to: "pdf-maker", label: "techpacks, proofs" },
    // Design state
    Edge { from: "design-state", to: "specs-service", label: "runtime specs" },
    Edge { from: "design-state", to: "asset-manager", label: "decoration assets" },
    Edge { from: "design-state", to: "multiplayer", label: "broadcast" },
    // Asset manager
    Edge { from: "asset-manager", to: "backend", label: "session validation" },
    Edge { from: "asset-manager", to: "design-state", label: "design reads" },
    // Fibra
    Edge { from: "fibra", to: "specs-service", label: "published specs" },
    Edge { from: "fibra", to: "backend", label: "customer auth" },
    Edge { from: "fibra", to: "distributors", label: "push / PromoStandards" },
    Edge { from: "prextra", to: "fibra", label: "product & inventory syncs" },
    // Specs
    Edge { from: "specs-service", to: "specs-baseline", label: "baseline merge" },
    Edge { from: "specs-service", to: "fibra", label: "publish sync" },
    // Packages
    Edge { from: "frontend", to: "webgl", label: "package" },
    Edge { from: "render-service", to: "webgl", label: "package" },
    Edge { from: "frontend", to: "specs-validation", label: "package" },
    Edge { from: "specs-service", to: "specs-validation", label: "package" },
    Edge { from: "frontend", to: "translations", label: "package" },
    Edge { from: "backend", to: "translations", label: "package" },
    Edge { from: "fibra", to: "translations", label: "package" },
    Edge { from: "specs-baseline", to: "translations", label: "label validation" },
];
