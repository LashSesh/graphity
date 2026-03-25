// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Domain knowledge system for ISLS v2.
//!
//! Pre-built domain templates (warehouse, ecommerce, project management)
//! provide rich entity definitions, business rules, and relationships
//! that are injected into the hypercube during TOML parsing.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

// ─── Field Definition ────────────────────────────────────────────────────────

/// Definition of a single entity field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldDef {
    /// Field name (snake_case).
    pub name: String,
    /// Rust type (e.g. "String", "i64", "Option<String>", "bool").
    pub rust_type: String,
    /// SQL column type (e.g. "BIGSERIAL", "VARCHAR(255)", "BIGINT").
    pub sql_type: String,
    /// Whether the field is nullable.
    pub nullable: bool,
    /// Default SQL value (e.g. "0", "NOW()", "'pending'").
    pub default_value: Option<String>,
    /// Human-readable description.
    pub description: String,
}

// ─── Validation Rule ─────────────────────────────────────────────────────────

/// A validation rule applied to an entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Rule name (e.g. "sku_not_empty").
    pub name: String,
    /// Rust boolean expression (e.g. "!self.sku.is_empty()").
    pub condition: String,
    /// Error message when validation fails.
    pub message: String,
}

// ─── Index Definition ────────────────────────────────────────────────────────

/// A database index on an entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexDef {
    /// Index name.
    pub name: String,
    /// Columns included in the index.
    pub columns: Vec<String>,
    /// Whether this is a unique index.
    pub unique: bool,
}

// ─── Entity Template ─────────────────────────────────────────────────────────

/// Full template for a domain entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityTemplate {
    /// Entity name (PascalCase, e.g. "Product").
    pub name: String,
    /// All fields including id and timestamps.
    pub fields: Vec<FieldDef>,
    /// Validation rules.
    pub validations: Vec<ValidationRule>,
    /// Database indexes.
    pub indices: Vec<IndexDef>,
    /// Human-readable description.
    pub description: String,
}

// ─── Relationship ────────────────────────────────────────────────────────────

/// Kind of relationship between entities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationshipKind {
    /// N:1 — child belongs to parent.
    BelongsTo,
    /// 1:N — parent has many children.
    HasMany,
    /// N:M — many-to-many through join table.
    ManyToMany,
}

/// ON DELETE behaviour for foreign keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnDelete {
    /// Delete children when parent is deleted.
    Cascade,
    /// Set foreign key to NULL when parent is deleted.
    SetNull,
    /// Prevent deletion of parent while children exist.
    Restrict,
}

/// A relationship between two entities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Relationship {
    /// Source entity name.
    pub from_entity: String,
    /// Target entity name.
    pub to_entity: String,
    /// Relationship kind.
    pub kind: RelationshipKind,
    /// Foreign key column name.
    pub foreign_key: String,
    /// ON DELETE behaviour.
    pub on_delete: OnDelete,
}

// ─── Business Rule ───────────────────────────────────────────────────────────

/// A business rule that generates service-layer logic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusinessRule {
    /// Rule name (e.g. "auto_reorder").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Trigger event (e.g. "on_create", "on_stock_change").
    pub trigger: String,
    /// Pseudocode description of the logic.
    pub logic_pseudocode: String,
    /// Service method name (e.g. "inventory_service::check_reorder_level").
    pub service_method: String,
    /// Entities involved in this rule.
    pub entities_involved: Vec<String>,
}

// ─── API Features ────────────────────────────────────────────────────────────

/// API feature configuration for a domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiFeatures {
    /// Enable pagination.
    pub pagination: bool,
    /// Filterable fields.
    pub filtering: Vec<String>,
    /// Sortable fields.
    pub sorting: Vec<String>,
    /// Full-text search fields (ILIKE).
    pub search_fields: Vec<String>,
    /// Export format support.
    pub export_formats: Vec<String>,
}

// ─── Domain Template ─────────────────────────────────────────────────────────

/// Complete domain knowledge template.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainTemplate {
    /// Domain name (e.g. "warehouse").
    pub name: String,
    /// Keywords that trigger domain detection.
    pub keywords: Vec<String>,
    /// All entity templates in this domain.
    pub entities: Vec<EntityTemplate>,
    /// Inter-entity relationships.
    pub relationships: Vec<Relationship>,
    /// Business rules.
    pub business_rules: Vec<BusinessRule>,
    /// API feature configuration.
    pub api_features: ApiFeatures,
}

// ─── Domain Registry ─────────────────────────────────────────────────────────

/// Registry of built-in domain templates.
pub struct DomainRegistry {
    domains: BTreeMap<String, DomainTemplate>,
}

impl DomainRegistry {
    /// Create a new registry pre-loaded with built-in domains.
    pub fn new() -> Self {
        let mut domains = BTreeMap::new();
        domains.insert("warehouse".into(), build_warehouse_domain());
        domains.insert("ecommerce".into(), build_ecommerce_domain());
        domains.insert("project_management".into(), build_pm_domain());
        Self { domains }
    }

    /// Look up a domain by name.
    pub fn get(&self, name: &str) -> Option<&DomainTemplate> {
        self.domains.get(name)
    }

    /// Detect the best-matching domain from a text description using keyword matching.
    pub fn detect(&self, text: &str) -> Option<&DomainTemplate> {
        let lower = text.to_lowercase();
        let mut best: Option<(&DomainTemplate, usize)> = None;
        for domain in self.domains.values() {
            let hits = domain
                .keywords
                .iter()
                .filter(|kw| lower.contains(kw.as_str()))
                .count();
            if hits > 0 {
                if best.map_or(true, |(_, prev)| hits > prev) {
                    best = Some((domain, hits));
                }
            }
        }
        best.map(|(d, _)| d)
    }

    /// All registered domain names.
    pub fn domain_names(&self) -> Vec<String> {
        self.domains.keys().cloned().collect()
    }
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Warehouse Domain ────────────────────────────────────────────────────────

fn build_warehouse_domain() -> DomainTemplate {
    DomainTemplate {
        name: "warehouse".into(),
        keywords: vec![
            "warehouse".into(), "inventory".into(), "stock".into(),
            "product".into(), "sku".into(), "reorder".into(),
            "shipment".into(), "fulfillment".into(), "logistics".into(),
        ],
        entities: vec![
            build_product_entity(),
            build_warehouse_entity(),
            build_supplier_entity(),
            build_order_entity(),
            build_order_item_entity(),
            build_stock_movement_entity(),
            build_user_entity(),
        ],
        relationships: vec![
            Relationship {
                from_entity: "Product".into(), to_entity: "Warehouse".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "warehouse_id".into(),
                on_delete: OnDelete::Restrict,
            },
            Relationship {
                from_entity: "Product".into(), to_entity: "Supplier".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "supplier_id".into(),
                on_delete: OnDelete::SetNull,
            },
            Relationship {
                from_entity: "Order".into(), to_entity: "Warehouse".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "warehouse_id".into(),
                on_delete: OnDelete::Restrict,
            },
            Relationship {
                from_entity: "OrderItem".into(), to_entity: "Order".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "order_id".into(),
                on_delete: OnDelete::Cascade,
            },
            Relationship {
                from_entity: "OrderItem".into(), to_entity: "Product".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "product_id".into(),
                on_delete: OnDelete::Restrict,
            },
            Relationship {
                from_entity: "StockMovement".into(), to_entity: "Product".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "product_id".into(),
                on_delete: OnDelete::Restrict,
            },
            Relationship {
                from_entity: "StockMovement".into(), to_entity: "Warehouse".into(),
                kind: RelationshipKind::BelongsTo, foreign_key: "warehouse_id".into(),
                on_delete: OnDelete::Restrict,
            },
        ],
        business_rules: vec![
            BusinessRule {
                name: "auto_reorder".into(),
                description: "When quantity_on_hand < reorder_level and product is active, log reorder warning".into(),
                trigger: "on_stock_change".into(),
                logic_pseudocode: "if product.quantity_on_hand < product.reorder_level && product.is_active { warn!(\"Reorder needed for {}\", product.sku); }".into(),
                service_method: "inventory_service::check_reorder_level".into(),
                entities_involved: vec!["Product".into()],
            },
            BusinessRule {
                name: "stock_on_fulfillment".into(),
                description: "When order status changes to 'shipped', create outbound StockMovements and decrease quantity".into(),
                trigger: "on_status_change".into(),
                logic_pseudocode: "for item in order.items { create_stock_movement(outbound, item.quantity); product.quantity_on_hand -= item.quantity; }".into(),
                service_method: "order_service::fulfill_order".into(),
                entities_involved: vec!["Order".into(), "OrderItem".into(), "Product".into(), "StockMovement".into()],
            },
            BusinessRule {
                name: "prevent_negative_stock".into(),
                description: "Outbound movements cannot reduce stock below zero".into(),
                trigger: "on_stock_change".into(),
                logic_pseudocode: "if product.quantity_on_hand - movement.quantity < 0 { return Err(\"insufficient stock\"); }".into(),
                service_method: "inventory_service::adjust_stock".into(),
                entities_involved: vec!["Product".into(), "StockMovement".into()],
            },
            BusinessRule {
                name: "order_total_calculation".into(),
                description: "Recalculate order total when items change".into(),
                trigger: "on_item_change".into(),
                logic_pseudocode: "order.total_amount_cents = order.items.iter().map(|i| i.quantity as i64 * i.unit_price_cents).sum();".into(),
                service_method: "order_service::recalculate_total".into(),
                entities_involved: vec!["Order".into(), "OrderItem".into()],
            },
            BusinessRule {
                name: "order_state_machine".into(),
                description: "Order status transitions: pending→confirmed→processing→shipped→delivered; any→cancelled (except delivered)".into(),
                trigger: "on_status_change".into(),
                logic_pseudocode: "match (current, next) { (pending, confirmed) | (confirmed, processing) | (processing, shipped) | (shipped, delivered) => Ok(()), (s, cancelled) if s != delivered => Ok(()), _ => Err(\"invalid transition\") }".into(),
                service_method: "order_service::update_status".into(),
                entities_involved: vec!["Order".into()],
            },
        ],
        api_features: ApiFeatures {
            pagination: true,
            filtering: vec!["status".into(), "category".into(), "warehouse_id".into(), "is_active".into(), "order_type".into(), "movement_type".into()],
            sorting: vec!["created_at".into(), "name".into(), "quantity_on_hand".into(), "unit_price_cents".into(), "order_number".into()],
            search_fields: vec!["name".into(), "sku".into(), "description".into()],
            export_formats: vec!["csv".into(), "json".into()],
        },
    }
}

fn build_product_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Product".into(),
        description: "Physical product tracked in the warehouse".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("sku", "String", "VARCHAR(100) NOT NULL UNIQUE", false, None, "Stock keeping unit"),
            f("name", "String", "VARCHAR(255) NOT NULL", false, None, "Product name"),
            f("description", "Option<String>", "TEXT", true, None, "Product description"),
            f("category", "Option<String>", "VARCHAR(100)", true, None, "Product category"),
            f("unit_price_cents", "i64", "BIGINT NOT NULL", false, Some("0"), "Selling price in cents"),
            f("cost_price_cents", "i64", "BIGINT NOT NULL", false, Some("0"), "Cost price in cents"),
            f("quantity_on_hand", "i32", "INTEGER NOT NULL", false, Some("0"), "Current stock quantity"),
            f("reorder_level", "i32", "INTEGER NOT NULL", false, Some("10"), "Minimum stock before reorder"),
            f("reorder_quantity", "i32", "INTEGER NOT NULL", false, Some("50"), "Quantity to reorder"),
            f("weight_grams", "Option<i32>", "INTEGER", true, None, "Weight in grams"),
            f("barcode", "Option<String>", "VARCHAR(100) UNIQUE", true, None, "Product barcode"),
            f("is_active", "bool", "BOOLEAN NOT NULL", false, Some("true"), "Whether product is active"),
            f("supplier_id", "Option<i64>", "BIGINT REFERENCES suppliers(id) ON DELETE SET NULL", true, None, "FK to supplier"),
            f("warehouse_id", "i64", "BIGINT NOT NULL REFERENCES warehouses(id)", false, None, "FK to warehouse"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Last update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "sku_not_empty".into(), condition: "!self.sku.trim().is_empty()".into(), message: "SKU must not be empty".into() },
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "price_non_negative".into(), condition: "self.unit_price_cents >= 0".into(), message: "Unit price must be non-negative".into() },
            ValidationRule { name: "cost_non_negative".into(), condition: "self.cost_price_cents >= 0".into(), message: "Cost price must be non-negative".into() },
            ValidationRule { name: "quantity_non_negative".into(), condition: "self.quantity_on_hand >= 0".into(), message: "Quantity must be non-negative".into() },
            ValidationRule { name: "reorder_positive".into(), condition: "self.reorder_level > 0".into(), message: "Reorder level must be positive".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_products_sku".into(), columns: vec!["sku".into()], unique: true },
            IndexDef { name: "idx_products_category".into(), columns: vec!["category".into()], unique: false },
            IndexDef { name: "idx_products_warehouse_id".into(), columns: vec!["warehouse_id".into()], unique: false },
            IndexDef { name: "idx_products_barcode".into(), columns: vec!["barcode".into()], unique: true },
        ],
    }
}

fn build_warehouse_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Warehouse".into(),
        description: "Physical warehouse location".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("name", "String", "VARCHAR(255) NOT NULL", false, None, "Warehouse name"),
            f("code", "String", "VARCHAR(50) NOT NULL UNIQUE", false, None, "Short code"),
            f("address", "Option<String>", "TEXT", true, None, "Street address"),
            f("city", "Option<String>", "VARCHAR(100)", true, None, "City"),
            f("country", "Option<String>", "VARCHAR(100)", true, None, "Country"),
            f("capacity_units", "Option<i32>", "INTEGER", true, None, "Max capacity units"),
            f("is_active", "bool", "BOOLEAN NOT NULL", false, Some("true"), "Active flag"),
            f("manager_name", "Option<String>", "VARCHAR(255)", true, None, "Manager name"),
            f("contact_email", "Option<String>", "VARCHAR(255)", true, None, "Contact email"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Last update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "code_not_empty".into(), condition: "!self.code.trim().is_empty()".into(), message: "Code must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_warehouses_code".into(), columns: vec!["code".into()], unique: true },
        ],
    }
}

fn build_supplier_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Supplier".into(),
        description: "Product supplier".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("name", "String", "VARCHAR(255) NOT NULL", false, None, "Supplier name"),
            f("contact_email", "Option<String>", "VARCHAR(255)", true, None, "Contact email"),
            f("contact_phone", "Option<String>", "VARCHAR(50)", true, None, "Contact phone"),
            f("address", "Option<String>", "TEXT", true, None, "Address"),
            f("lead_time_days", "i32", "INTEGER NOT NULL", false, Some("7"), "Lead time in days"),
            f("is_active", "bool", "BOOLEAN NOT NULL", false, Some("true"), "Active flag"),
            f("rating", "Option<i32>", "INTEGER CHECK (rating >= 1 AND rating <= 5)", true, None, "Rating 1-5"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Last update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_suppliers_name".into(), columns: vec!["name".into()], unique: false },
        ],
    }
}

fn build_order_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Order".into(),
        description: "Inbound or outbound order".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("order_number", "String", "VARCHAR(100) NOT NULL UNIQUE", false, None, "Unique order number"),
            f("status", "String", "VARCHAR(50) NOT NULL", false, Some("'pending'"), "Order status"),
            f("order_type", "String", "VARCHAR(50) NOT NULL", false, None, "Type: inbound/outbound/transfer"),
            f("customer_name", "Option<String>", "VARCHAR(255)", true, None, "Customer name"),
            f("customer_email", "Option<String>", "VARCHAR(255)", true, None, "Customer email"),
            f("warehouse_id", "i64", "BIGINT NOT NULL REFERENCES warehouses(id)", false, None, "FK to warehouse"),
            f("total_amount_cents", "i64", "BIGINT NOT NULL", false, Some("0"), "Total in cents"),
            f("notes", "Option<String>", "TEXT", true, None, "Order notes"),
            f("shipped_at", "Option<String>", "TIMESTAMPTZ", true, None, "Ship timestamp"),
            f("delivered_at", "Option<String>", "TIMESTAMPTZ", true, None, "Delivery timestamp"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Last update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"pending\" | \"confirmed\" | \"processing\" | \"shipped\" | \"delivered\" | \"cancelled\")".into(), message: "Invalid order status".into() },
            ValidationRule { name: "valid_order_type".into(), condition: "matches!(self.order_type.as_str(), \"inbound\" | \"outbound\" | \"transfer\")".into(), message: "Invalid order type".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_orders_order_number".into(), columns: vec!["order_number".into()], unique: true },
            IndexDef { name: "idx_orders_status".into(), columns: vec!["status".into()], unique: false },
            IndexDef { name: "idx_orders_warehouse_id".into(), columns: vec!["warehouse_id".into()], unique: false },
        ],
    }
}

fn build_order_item_entity() -> EntityTemplate {
    EntityTemplate {
        name: "OrderItem".into(),
        description: "Line item within an order".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("order_id", "i64", "BIGINT NOT NULL REFERENCES orders(id) ON DELETE CASCADE", false, None, "FK to order"),
            f("product_id", "i64", "BIGINT NOT NULL REFERENCES products(id)", false, None, "FK to product"),
            f("quantity", "i32", "INTEGER NOT NULL", false, None, "Item quantity"),
            f("unit_price_cents", "i64", "BIGINT NOT NULL", false, None, "Unit price at time of order"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "quantity_positive".into(), condition: "self.quantity > 0".into(), message: "Quantity must be positive".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_order_items_order_id".into(), columns: vec!["order_id".into()], unique: false },
            IndexDef { name: "idx_order_items_product_id".into(), columns: vec!["product_id".into()], unique: false },
        ],
    }
}

fn build_stock_movement_entity() -> EntityTemplate {
    EntityTemplate {
        name: "StockMovement".into(),
        description: "Record of inventory change".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("product_id", "i64", "BIGINT NOT NULL REFERENCES products(id)", false, None, "FK to product"),
            f("warehouse_id", "i64", "BIGINT NOT NULL REFERENCES warehouses(id)", false, None, "FK to warehouse"),
            f("movement_type", "String", "VARCHAR(50) NOT NULL", false, None, "Type: receipt/shipment/adjustment/transfer/return"),
            f("quantity", "i32", "INTEGER NOT NULL", false, None, "Signed quantity change"),
            f("reference_type", "Option<String>", "VARCHAR(50)", true, None, "Reference entity type"),
            f("reference_id", "Option<i64>", "BIGINT", true, None, "Reference entity ID"),
            f("notes", "Option<String>", "TEXT", true, None, "Movement notes"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "valid_movement_type".into(), condition: "matches!(self.movement_type.as_str(), \"receipt\" | \"shipment\" | \"adjustment\" | \"transfer\" | \"return\")".into(), message: "Invalid movement type".into() },
            ValidationRule { name: "nonzero_quantity".into(), condition: "self.quantity != 0".into(), message: "Quantity must be nonzero".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_stock_movements_product_id".into(), columns: vec!["product_id".into()], unique: false },
            IndexDef { name: "idx_stock_movements_warehouse_id".into(), columns: vec!["warehouse_id".into()], unique: false },
            IndexDef { name: "idx_stock_movements_type".into(), columns: vec!["movement_type".into()], unique: false },
        ],
    }
}

fn build_user_entity() -> EntityTemplate {
    EntityTemplate {
        name: "User".into(),
        description: "System user with role-based access".into(),
        fields: vec![
            f("id", "i64", "BIGSERIAL PRIMARY KEY", false, None, "Primary key"),
            f("email", "String", "VARCHAR(255) NOT NULL UNIQUE", false, None, "User email"),
            f("password_hash", "String", "VARCHAR(255) NOT NULL", false, None, "Bcrypt password hash"),
            f("name", "String", "VARCHAR(255) NOT NULL", false, None, "Display name"),
            f("role", "String", "VARCHAR(50) NOT NULL", false, Some("'operator'"), "Role: admin/manager/operator/viewer"),
            f("is_active", "bool", "BOOLEAN NOT NULL", false, Some("true"), "Active flag"),
            f("last_login_at", "Option<String>", "TIMESTAMPTZ", true, None, "Last login timestamp"),
            f("created_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at", "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Last update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "email_not_empty".into(), condition: "!self.email.trim().is_empty()".into(), message: "Email must not be empty".into() },
            ValidationRule { name: "valid_role".into(), condition: "matches!(self.role.as_str(), \"admin\" | \"manager\" | \"operator\" | \"viewer\")".into(), message: "Invalid role".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_users_email".into(), columns: vec!["email".into()], unique: true },
        ],
    }
}

// ─── Ecommerce Domain (minimal) ─────────────────────────────────────────────

fn build_ecommerce_domain() -> DomainTemplate {
    DomainTemplate {
        name: "ecommerce".into(),
        keywords: vec!["ecommerce".into(), "cart".into(), "checkout".into(), "payment".into(), "shop".into()],
        entities: vec![],
        relationships: vec![],
        business_rules: vec![],
        api_features: ApiFeatures {
            pagination: true,
            filtering: vec!["status".into(), "category".into()],
            sorting: vec!["created_at".into(), "price".into()],
            search_fields: vec!["name".into(), "description".into()],
            export_formats: vec!["json".into()],
        },
    }
}

// ─── Project Management Domain (minimal) ─────────────────────────────────────

fn build_pm_domain() -> DomainTemplate {
    DomainTemplate {
        name: "project_management".into(),
        keywords: vec!["project".into(), "task".into(), "sprint".into(), "milestone".into(), "kanban".into()],
        entities: vec![],
        relationships: vec![],
        business_rules: vec![],
        api_features: ApiFeatures {
            pagination: true,
            filtering: vec!["status".into(), "priority".into(), "assignee".into()],
            sorting: vec!["created_at".into(), "due_date".into(), "priority".into()],
            search_fields: vec!["title".into(), "description".into()],
            export_formats: vec!["json".into(), "csv".into()],
        },
    }
}

// ─── Helper ──────────────────────────────────────────────────────────────────

fn f(name: &str, rust_type: &str, sql_type: &str, nullable: bool, default: Option<&str>, desc: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        rust_type: rust_type.into(),
        sql_type: sql_type.into(),
        nullable,
        default_value: default.map(Into::into),
        description: desc.into(),
    }
}
