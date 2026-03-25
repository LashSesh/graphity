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

// ─── Ecommerce Domain ────────────────────────────────────────────────────────

fn build_ecommerce_domain() -> DomainTemplate {
    DomainTemplate {
        name: "ecommerce".into(),
        keywords: vec![
            "ecommerce".into(), "e-commerce".into(), "shop".into(), "store".into(),
            "cart".into(), "checkout".into(), "payment".into(), "product".into(),
            "order".into(), "customer".into(), "catalog".into(),
        ],
        entities: vec![
            ec_category_entity(),
            ec_product_entity(),
            ec_customer_entity(),
            ec_address_entity(),
            ec_cart_entity(),
            ec_cart_item_entity(),
            ec_order_entity(),
            ec_order_line_entity(),
            ec_review_entity(),
        ],
        relationships: vec![
            Relationship { from_entity: "Product".into(),  to_entity: "Category".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "category_id".into(),  on_delete: OnDelete::SetNull },
            Relationship { from_entity: "Address".into(),  to_entity: "Customer".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "customer_id".into(),  on_delete: OnDelete::Cascade },
            Relationship { from_entity: "Cart".into(),     to_entity: "Customer".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "customer_id".into(),  on_delete: OnDelete::Cascade },
            Relationship { from_entity: "CartItem".into(), to_entity: "Cart".into(),      kind: RelationshipKind::BelongsTo, foreign_key: "cart_id".into(),      on_delete: OnDelete::Cascade },
            Relationship { from_entity: "CartItem".into(), to_entity: "Product".into(),   kind: RelationshipKind::BelongsTo, foreign_key: "product_id".into(),   on_delete: OnDelete::Restrict },
            Relationship { from_entity: "Order".into(),    to_entity: "Customer".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "customer_id".into(),  on_delete: OnDelete::Restrict },
            Relationship { from_entity: "Order".into(),    to_entity: "Address".into(),   kind: RelationshipKind::BelongsTo, foreign_key: "shipping_address_id".into(), on_delete: OnDelete::SetNull },
            Relationship { from_entity: "OrderLine".into(), to_entity: "Order".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "order_id".into(),     on_delete: OnDelete::Cascade },
            Relationship { from_entity: "OrderLine".into(), to_entity: "Product".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "product_id".into(),   on_delete: OnDelete::Restrict },
            Relationship { from_entity: "Review".into(),   to_entity: "Product".into(),   kind: RelationshipKind::BelongsTo, foreign_key: "product_id".into(),   on_delete: OnDelete::Cascade },
            Relationship { from_entity: "Review".into(),   to_entity: "Customer".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "customer_id".into(),  on_delete: OnDelete::Cascade },
        ],
        business_rules: vec![
            BusinessRule {
                name: "cart_to_order".into(),
                description: "Convert active cart to order, clearing cart items".into(),
                trigger: "on_checkout".into(),
                logic_pseudocode: "order = create_order(cart); for item in cart.items { create_order_line(order, item); } cart.status = 'checked_out';".into(),
                service_method: "cart_service::checkout".into(),
                entities_involved: vec!["Cart".into(), "CartItem".into(), "Order".into(), "OrderLine".into()],
            },
            BusinessRule {
                name: "inventory_check".into(),
                description: "Verify product inventory_count >= quantity before adding to cart".into(),
                trigger: "on_add_to_cart".into(),
                logic_pseudocode: "if product.inventory_count < quantity { return Err(\"insufficient stock\"); }".into(),
                service_method: "cart_service::add_item".into(),
                entities_involved: vec!["Product".into(), "CartItem".into()],
            },
            BusinessRule {
                name: "order_state_machine".into(),
                description: "Order status: pending→confirmed→shipped→delivered; any→cancelled (except delivered)".into(),
                trigger: "on_status_change".into(),
                logic_pseudocode: "match (current, next) { (pending, confirmed) | (confirmed, shipped) | (shipped, delivered) => Ok(()), (s, cancelled) if s != delivered => Ok(()), _ => Err(\"invalid transition\") }".into(),
                service_method: "order_service::update_status".into(),
                entities_involved: vec!["Order".into()],
            },
            BusinessRule {
                name: "review_moderation".into(),
                description: "New reviews default to is_approved=false; admin must approve".into(),
                trigger: "on_create".into(),
                logic_pseudocode: "review.is_approved = false;".into(),
                service_method: "review_service::create".into(),
                entities_involved: vec!["Review".into()],
            },
            BusinessRule {
                name: "no_duplicate_review".into(),
                description: "One review per customer per product".into(),
                trigger: "on_create".into(),
                logic_pseudocode: "if exists(review where product_id=? AND customer_id=?) { return Err(\"already reviewed\"); }".into(),
                service_method: "review_service::create".into(),
                entities_involved: vec!["Review".into()],
            },
        ],
        api_features: ApiFeatures {
            pagination: true,
            filtering: vec!["status".into(), "category_id".into(), "customer_id".into(), "is_published".into(), "payment_status".into(), "is_approved".into()],
            sorting: vec!["created_at".into(), "price_cents".into(), "name".into(), "rating".into()],
            search_fields: vec!["name".into(), "sku".into(), "description".into(), "email".into()],
            export_formats: vec!["csv".into(), "json".into()],
        },
    }
}

fn ec_category_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Category".into(),
        description: "Product category (self-referential tree)".into(),
        fields: vec![
            f("id",        "i64",           "BIGSERIAL PRIMARY KEY",                             false, None,           "Primary key"),
            f("name",      "String",        "VARCHAR(255) NOT NULL",                              false, None,           "Category name"),
            f("slug",      "String",        "VARCHAR(255) NOT NULL UNIQUE",                       false, None,           "URL slug"),
            f("parent_id", "Option<i64>",   "BIGINT REFERENCES categories(id) ON DELETE SET NULL", true, None,          "Parent category FK"),
            f("position",  "i32",           "INTEGER NOT NULL",                                   false, Some("0"),     "Sort position"),
            f("is_active", "bool",          "BOOLEAN NOT NULL",                                   false, Some("true"), "Active flag"),
            f("created_at","String",        "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                 false, Some("NOW()"), "Creation timestamp"),
            f("updated_at","String",        "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                 false, Some("NOW()"), "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "slug_not_empty".into(), condition: "!self.slug.trim().is_empty()".into(), message: "Slug must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_categories_slug".into(),   columns: vec!["slug".into()],      unique: true },
            IndexDef { name: "idx_categories_parent".into(), columns: vec!["parent_id".into()], unique: false },
        ],
    }
}

fn ec_product_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Product".into(),
        description: "Sellable product in the e-commerce catalog".into(),
        fields: vec![
            f("id",              "i64",           "BIGSERIAL PRIMARY KEY",                              false, None,           "Primary key"),
            f("name",            "String",        "VARCHAR(255) NOT NULL",                               false, None,           "Product name"),
            f("slug",            "String",        "VARCHAR(255) NOT NULL UNIQUE",                        false, None,           "URL slug"),
            f("price_cents",     "i64",           "BIGINT NOT NULL",                                     false, Some("0"),     "Selling price in cents"),
            f("compare_price",   "Option<i64>",   "BIGINT",                                              true,  None,           "Original/compare price in cents"),
            f("sku",             "String",        "VARCHAR(100) NOT NULL UNIQUE",                        false, None,           "Stock keeping unit"),
            f("inventory_count", "i32",           "INTEGER NOT NULL",                                    false, Some("0"),     "Available inventory"),
            f("category_id",     "Option<i64>",   "BIGINT REFERENCES categories(id) ON DELETE SET NULL", true,  None,          "FK to category"),
            f("is_published",    "bool",          "BOOLEAN NOT NULL",                                    false, Some("false"), "Visible in storefront"),
            f("image_url",       "Option<String>","VARCHAR(1024)",                                       true,  None,           "Main image URL"),
            f("weight_grams",    "Option<i32>",   "INTEGER",                                             true,  None,           "Weight for shipping"),
            f("description",     "Option<String>","TEXT",                                                true,  None,           "Product description"),
            f("created_at",      "String",        "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                  false, Some("NOW()"), "Creation timestamp"),
            f("updated_at",      "String",        "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                  false, Some("NOW()"), "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(),  condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "sku_not_empty".into(),   condition: "!self.sku.trim().is_empty()".into(),  message: "SKU must not be empty".into() },
            ValidationRule { name: "price_nonneg".into(),    condition: "self.price_cents >= 0".into(),        message: "Price must be non-negative".into() },
            ValidationRule { name: "inventory_nonneg".into(),condition: "self.inventory_count >= 0".into(),    message: "Inventory must be non-negative".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_ec_products_slug".into(),     columns: vec!["slug".into()],        unique: true },
            IndexDef { name: "idx_ec_products_sku".into(),      columns: vec!["sku".into()],         unique: true },
            IndexDef { name: "idx_ec_products_category".into(), columns: vec!["category_id".into()], unique: false },
        ],
    }
}

fn ec_customer_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Customer".into(),
        description: "Registered customer account".into(),
        fields: vec![
            f("id",            "i64",          "BIGSERIAL PRIMARY KEY",         false, None,           "Primary key"),
            f("email",         "String",       "VARCHAR(255) NOT NULL UNIQUE",   false, None,           "Customer email"),
            f("password_hash", "String",       "VARCHAR(255) NOT NULL",          false, None,           "Bcrypt password hash"),
            f("first_name",    "String",       "VARCHAR(100) NOT NULL",          false, None,           "First name"),
            f("last_name",     "String",       "VARCHAR(100) NOT NULL",          false, None,           "Last name"),
            f("phone",         "Option<String>","VARCHAR(50)",                   true,  None,           "Phone number"),
            f("is_active",     "bool",         "BOOLEAN NOT NULL",               false, Some("true"), "Active flag"),
            f("created_at",    "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at",    "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "email_not_empty".into(),     condition: "!self.email.trim().is_empty()".into(),      message: "Email must not be empty".into() },
            ValidationRule { name: "first_name_not_empty".into(),condition: "!self.first_name.trim().is_empty()".into(), message: "First name must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_customers_email".into(), columns: vec!["email".into()], unique: true },
        ],
    }
}

fn ec_address_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Address".into(),
        description: "Customer shipping/billing address".into(),
        fields: vec![
            f("id",          "i64",    "BIGSERIAL PRIMARY KEY",                               false, None,           "Primary key"),
            f("customer_id", "i64",    "BIGINT NOT NULL REFERENCES customers(id) ON DELETE CASCADE", false, None,    "FK to customer"),
            f("label",       "String", "VARCHAR(100) NOT NULL",                                false, None,           "Label (Home, Work…)"),
            f("street",      "String", "VARCHAR(255) NOT NULL",                                false, None,           "Street address"),
            f("city",        "String", "VARCHAR(100) NOT NULL",                                false, None,           "City"),
            f("state",       "Option<String>","VARCHAR(100)",                                  true,  None,           "State/province"),
            f("postal_code", "String", "VARCHAR(20) NOT NULL",                                 false, None,           "Postal code"),
            f("country",     "String", "VARCHAR(100) NOT NULL",                                false, Some("'US'"),  "ISO country code"),
            f("is_default",  "bool",   "BOOLEAN NOT NULL",                                     false, Some("false"), "Default address flag"),
            f("created_at",  "String", "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                   false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "street_not_empty".into(), condition: "!self.street.trim().is_empty()".into(), message: "Street must not be empty".into() },
            ValidationRule { name: "city_not_empty".into(),   condition: "!self.city.trim().is_empty()".into(),   message: "City must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_addresses_customer".into(), columns: vec!["customer_id".into()], unique: false },
        ],
    }
}

fn ec_cart_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Cart".into(),
        description: "Shopping cart for a customer session".into(),
        fields: vec![
            f("id",          "i64",          "BIGSERIAL PRIMARY KEY",                                    false, None,              "Primary key"),
            f("customer_id", "Option<i64>",  "BIGINT REFERENCES customers(id) ON DELETE CASCADE",        true,  None,              "FK to customer (null for guest)"),
            f("session_id",  "String",       "VARCHAR(255) NOT NULL",                                    false, None,              "Session identifier"),
            f("status",      "String",       "VARCHAR(50) NOT NULL",                                     false, Some("'active'"), "active | checked_out | abandoned"),
            f("created_at",  "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                       false, Some("NOW()"),    "Creation timestamp"),
            f("updated_at",  "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                       false, Some("NOW()"),    "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"active\" | \"checked_out\" | \"abandoned\")".into(), message: "Invalid cart status".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_carts_customer".into(), columns: vec!["customer_id".into()], unique: false },
            IndexDef { name: "idx_carts_session".into(),  columns: vec!["session_id".into()],  unique: false },
        ],
    }
}

fn ec_cart_item_entity() -> EntityTemplate {
    EntityTemplate {
        name: "CartItem".into(),
        description: "Line item within a shopping cart".into(),
        fields: vec![
            f("id",             "i64", "BIGSERIAL PRIMARY KEY",                                      false, None,       "Primary key"),
            f("cart_id",        "i64", "BIGINT NOT NULL REFERENCES carts(id) ON DELETE CASCADE",     false, None,       "FK to cart"),
            f("product_id",     "i64", "BIGINT NOT NULL REFERENCES products(id) ON DELETE RESTRICT", false, None,       "FK to product"),
            f("quantity",       "i32", "INTEGER NOT NULL",                                            false, Some("1"), "Quantity"),
            f("unit_price_cents","i64","BIGINT NOT NULL",                                             false, Some("0"), "Price at time of adding"),
            f("created_at",     "String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                        false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "qty_positive".into(), condition: "self.quantity > 0".into(), message: "Quantity must be positive".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_cart_items_cart".into(),    columns: vec!["cart_id".into()],    unique: false },
            IndexDef { name: "idx_cart_items_product".into(), columns: vec!["product_id".into()], unique: false },
        ],
    }
}

fn ec_order_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Order".into(),
        description: "Placed customer order".into(),
        fields: vec![
            f("id",                  "i64",          "BIGSERIAL PRIMARY KEY",                                         false, None,               "Primary key"),
            f("order_number",        "String",       "VARCHAR(100) NOT NULL UNIQUE",                                  false, None,               "Unique order reference"),
            f("customer_id",         "Option<i64>",  "BIGINT REFERENCES customers(id) ON DELETE SET NULL",            true,  None,               "FK to customer"),
            f("status",              "String",       "VARCHAR(50) NOT NULL",                                          false, Some("'pending'"), "Order status"),
            f("subtotal",            "i64",          "BIGINT NOT NULL",                                               false, Some("0"),         "Subtotal in cents"),
            f("tax",                 "i64",          "BIGINT NOT NULL",                                               false, Some("0"),         "Tax in cents"),
            f("shipping",            "i64",          "BIGINT NOT NULL",                                               false, Some("0"),         "Shipping in cents"),
            f("total",               "i64",          "BIGINT NOT NULL",                                               false, Some("0"),         "Total in cents"),
            f("payment_status",      "String",       "VARCHAR(50) NOT NULL",                                          false, Some("'unpaid'"), "Payment status"),
            f("shipping_address_id", "Option<i64>",  "BIGINT REFERENCES addresses(id) ON DELETE SET NULL",            true,  None,               "FK to shipping address"),
            f("notes",               "Option<String>","TEXT",                                                          true,  None,               "Order notes"),
            f("created_at",          "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                            false, Some("NOW()"),     "Creation timestamp"),
            f("updated_at",          "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                            false, Some("NOW()"),     "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"pending\" | \"confirmed\" | \"shipped\" | \"delivered\" | \"cancelled\")".into(), message: "Invalid order status".into() },
            ValidationRule { name: "valid_payment".into(), condition: "matches!(self.payment_status.as_str(), \"unpaid\" | \"paid\" | \"refunded\" | \"failed\")".into(), message: "Invalid payment status".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_ec_orders_number".into(),   columns: vec!["order_number".into()],  unique: true },
            IndexDef { name: "idx_ec_orders_customer".into(), columns: vec!["customer_id".into()],   unique: false },
            IndexDef { name: "idx_ec_orders_status".into(),   columns: vec!["status".into()],        unique: false },
        ],
    }
}

fn ec_order_line_entity() -> EntityTemplate {
    EntityTemplate {
        name: "OrderLine".into(),
        description: "Line item within a placed order".into(),
        fields: vec![
            f("id",             "i64", "BIGSERIAL PRIMARY KEY",                                      false, None,       "Primary key"),
            f("order_id",       "i64", "BIGINT NOT NULL REFERENCES orders(id) ON DELETE CASCADE",    false, None,       "FK to order"),
            f("product_id",     "i64", "BIGINT NOT NULL REFERENCES products(id) ON DELETE RESTRICT", false, None,       "FK to product"),
            f("quantity",       "i32", "INTEGER NOT NULL",                                            false, None,       "Quantity ordered"),
            f("unit_price_cents","i64","BIGINT NOT NULL",                                             false, None,       "Unit price at time of order"),
            f("created_at",     "String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                        false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "qty_positive".into(), condition: "self.quantity > 0".into(), message: "Quantity must be positive".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_order_lines_order".into(),   columns: vec!["order_id".into()],   unique: false },
            IndexDef { name: "idx_order_lines_product".into(), columns: vec!["product_id".into()], unique: false },
        ],
    }
}

fn ec_review_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Review".into(),
        description: "Customer product review".into(),
        fields: vec![
            f("id",          "i64",          "BIGSERIAL PRIMARY KEY",                                          false, None,            "Primary key"),
            f("product_id",  "i64",          "BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE",      false, None,            "FK to product"),
            f("customer_id", "i64",          "BIGINT NOT NULL REFERENCES customers(id) ON DELETE CASCADE",     false, None,            "FK to customer"),
            f("rating",      "i32",          "INTEGER NOT NULL CHECK (rating >= 1 AND rating <= 5)",           false, None,            "Rating 1-5"),
            f("title",       "Option<String>","VARCHAR(255)",                                                   true,  None,            "Review title"),
            f("body",        "Option<String>","TEXT",                                                           true,  None,            "Review body"),
            f("is_approved", "bool",         "BOOLEAN NOT NULL",                                               false, Some("false"), "Approved by moderator"),
            f("created_at",  "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                             false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "rating_range".into(), condition: "self.rating >= 1 && self.rating <= 5".into(), message: "Rating must be 1-5".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_reviews_product".into(),  columns: vec!["product_id".into()],  unique: false },
            IndexDef { name: "idx_reviews_customer".into(), columns: vec!["customer_id".into()], unique: false },
            IndexDef { name: "idx_reviews_approved".into(), columns: vec!["is_approved".into()], unique: false },
        ],
    }
}

// ─── Project Management Domain ───────────────────────────────────────────────

fn build_pm_domain() -> DomainTemplate {
    DomainTemplate {
        name: "project_management".into(),
        keywords: vec![
            "project".into(), "task".into(), "sprint".into(), "milestone".into(),
            "kanban".into(), "scrum".into(), "agile".into(), "backlog".into(),
            "issue".into(), "ticket".into(), "tracker".into(), "jira".into(),
        ],
        entities: vec![
            pm_user_entity(),
            pm_project_entity(),
            pm_sprint_entity(),
            pm_task_entity(),
            pm_comment_entity(),
            pm_label_entity(),
            pm_task_label_entity(),
            pm_team_member_entity(),
        ],
        relationships: vec![
            Relationship { from_entity: "Project".into(),    to_entity: "User".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "owner_id".into(),    on_delete: OnDelete::Restrict },
            Relationship { from_entity: "Sprint".into(),     to_entity: "Project".into(), kind: RelationshipKind::BelongsTo, foreign_key: "project_id".into(),  on_delete: OnDelete::Cascade },
            Relationship { from_entity: "Task".into(),       to_entity: "Project".into(), kind: RelationshipKind::BelongsTo, foreign_key: "project_id".into(),  on_delete: OnDelete::Cascade },
            Relationship { from_entity: "Task".into(),       to_entity: "Sprint".into(),  kind: RelationshipKind::BelongsTo, foreign_key: "sprint_id".into(),   on_delete: OnDelete::SetNull },
            Relationship { from_entity: "Task".into(),       to_entity: "User".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "assignee_id".into(),  on_delete: OnDelete::SetNull },
            Relationship { from_entity: "Comment".into(),    to_entity: "Task".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "task_id".into(),     on_delete: OnDelete::Cascade },
            Relationship { from_entity: "Comment".into(),    to_entity: "User".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "author_id".into(),   on_delete: OnDelete::Restrict },
            Relationship { from_entity: "Label".into(),      to_entity: "Project".into(), kind: RelationshipKind::BelongsTo, foreign_key: "project_id".into(),  on_delete: OnDelete::Cascade },
            Relationship { from_entity: "TaskLabel".into(),  to_entity: "Task".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "task_id".into(),     on_delete: OnDelete::Cascade },
            Relationship { from_entity: "TaskLabel".into(),  to_entity: "Label".into(),   kind: RelationshipKind::BelongsTo, foreign_key: "label_id".into(),    on_delete: OnDelete::Cascade },
            Relationship { from_entity: "TeamMember".into(), to_entity: "User".into(),    kind: RelationshipKind::BelongsTo, foreign_key: "user_id".into(),     on_delete: OnDelete::Cascade },
            Relationship { from_entity: "TeamMember".into(), to_entity: "Project".into(), kind: RelationshipKind::BelongsTo, foreign_key: "project_id".into(),  on_delete: OnDelete::Cascade },
        ],
        business_rules: vec![
            BusinessRule {
                name: "task_state_machine".into(),
                description: "Task status: todo→in_progress→in_review→done; any→blocked".into(),
                trigger: "on_status_change".into(),
                logic_pseudocode: "match (current, next) { (todo, in_progress) | (in_progress, in_review) | (in_review, done) | (_, blocked) => Ok(()), _ => Err(\"invalid transition\") }".into(),
                service_method: "task_service::update_status".into(),
                entities_involved: vec!["Task".into()],
            },
            BusinessRule {
                name: "sprint_velocity".into(),
                description: "Calculate sprint velocity as sum of estimate_hours for completed tasks".into(),
                trigger: "on_sprint_close".into(),
                logic_pseudocode: "velocity = tasks.filter(status == done).sum(estimate_hours);".into(),
                service_method: "sprint_service::calculate_velocity".into(),
                entities_involved: vec!["Sprint".into(), "Task".into()],
            },
            BusinessRule {
                name: "role_based_assignment".into(),
                description: "Only project members can be assigned tasks".into(),
                trigger: "on_task_assign".into(),
                logic_pseudocode: "if !team_members.contains(assignee_id) { return Err(\"not a project member\"); }".into(),
                service_method: "task_service::assign".into(),
                entities_involved: vec!["Task".into(), "TeamMember".into()],
            },
        ],
        api_features: ApiFeatures {
            pagination: true,
            filtering: vec!["status".into(), "priority".into(), "assignee_id".into(), "sprint_id".into(), "project_id".into()],
            sorting: vec!["created_at".into(), "due_date".into(), "priority".into(), "position".into()],
            search_fields: vec!["title".into(), "description".into(), "name".into()],
            export_formats: vec!["json".into(), "csv".into()],
        },
    }
}

fn pm_user_entity() -> EntityTemplate {
    EntityTemplate {
        name: "User".into(),
        description: "System user".into(),
        fields: vec![
            f("id",            "i64",          "BIGSERIAL PRIMARY KEY",           false, None,           "Primary key"),
            f("email",         "String",       "VARCHAR(255) NOT NULL UNIQUE",     false, None,           "User email"),
            f("password_hash", "String",       "VARCHAR(255) NOT NULL",            false, None,           "Bcrypt hash"),
            f("display_name",  "String",       "VARCHAR(255) NOT NULL",            false, None,           "Display name"),
            f("avatar_url",    "Option<String>","VARCHAR(1024)",                   true,  None,           "Avatar URL"),
            f("is_active",     "bool",         "BOOLEAN NOT NULL",                 false, Some("true"), "Active flag"),
            f("created_at",    "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Creation timestamp"),
            f("updated_at",    "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()", false, Some("NOW()"), "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "email_not_empty".into(),   condition: "!self.email.trim().is_empty()".into(),        message: "Email must not be empty".into() },
            ValidationRule { name: "name_not_empty".into(),    condition: "!self.display_name.trim().is_empty()".into(), message: "Display name must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_pm_users_email".into(), columns: vec!["email".into()], unique: true },
        ],
    }
}

fn pm_project_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Project".into(),
        description: "Top-level project container".into(),
        fields: vec![
            f("id",             "i64",          "BIGSERIAL PRIMARY KEY",                              false, None,              "Primary key"),
            f("name",           "String",       "VARCHAR(255) NOT NULL",                               false, None,              "Project name"),
            f("description",    "Option<String>","TEXT",                                               true,  None,              "Project description"),
            f("owner_id",       "i64",          "BIGINT NOT NULL REFERENCES users(id)",                false, None,              "FK to owner"),
            f("status",         "String",       "VARCHAR(50) NOT NULL",                                false, Some("'active'"), "active | archived | completed"),
            f("start_date",     "Option<String>","DATE",                                               true,  None,              "Start date"),
            f("target_end_date","Option<String>","DATE",                                               true,  None,              "Target end date"),
            f("created_at",     "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                  false, Some("NOW()"),    "Creation timestamp"),
            f("updated_at",     "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                  false, Some("NOW()"),    "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"active\" | \"archived\" | \"completed\")".into(), message: "Invalid project status".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_projects_owner".into(),  columns: vec!["owner_id".into()],  unique: false },
            IndexDef { name: "idx_projects_status".into(), columns: vec!["status".into()],    unique: false },
        ],
    }
}

fn pm_sprint_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Sprint".into(),
        description: "Time-boxed sprint within a project".into(),
        fields: vec![
            f("id",         "i64",          "BIGSERIAL PRIMARY KEY",                                  false, None,              "Primary key"),
            f("project_id", "i64",          "BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE", false, None,           "FK to project"),
            f("name",       "String",       "VARCHAR(255) NOT NULL",                                   false, None,              "Sprint name"),
            f("goal",       "Option<String>","TEXT",                                                   true,  None,              "Sprint goal"),
            f("start_date", "Option<String>","DATE",                                                   true,  None,              "Start date"),
            f("end_date",   "Option<String>","DATE",                                                   true,  None,              "End date"),
            f("status",     "String",       "VARCHAR(50) NOT NULL",                                    false, Some("'planned'"),"planned | active | completed"),
            f("created_at", "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                      false, Some("NOW()"),    "Creation timestamp"),
            f("updated_at", "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                      false, Some("NOW()"),    "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Name must not be empty".into() },
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"planned\" | \"active\" | \"completed\")".into(), message: "Invalid sprint status".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_sprints_project".into(), columns: vec!["project_id".into()], unique: false },
            IndexDef { name: "idx_sprints_status".into(),  columns: vec!["status".into()],     unique: false },
        ],
    }
}

fn pm_task_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Task".into(),
        description: "Work item within a project".into(),
        fields: vec![
            f("id",             "i64",          "BIGSERIAL PRIMARY KEY",                                   false, None,              "Primary key"),
            f("project_id",     "i64",          "BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE",false, None,             "FK to project"),
            f("sprint_id",      "Option<i64>",  "BIGINT REFERENCES sprints(id) ON DELETE SET NULL",        true,  None,              "FK to sprint"),
            f("title",          "String",       "VARCHAR(500) NOT NULL",                                   false, None,              "Task title"),
            f("description",    "Option<String>","TEXT",                                                   true,  None,              "Task description"),
            f("assignee_id",    "Option<i64>",  "BIGINT REFERENCES users(id) ON DELETE SET NULL",          true,  None,              "FK to assignee"),
            f("reporter_id",    "i64",          "BIGINT NOT NULL REFERENCES users(id)",                    false, None,              "FK to reporter"),
            f("status",         "String",       "VARCHAR(50) NOT NULL",                                    false, Some("'todo'"),   "todo|in_progress|in_review|done|blocked"),
            f("priority",       "String",       "VARCHAR(50) NOT NULL",                                    false, Some("'medium'"),"low|medium|high|critical"),
            f("estimate_hours", "Option<f64>",  "NUMERIC(6,2)",                                            true,  None,              "Estimated hours"),
            f("actual_hours",   "Option<f64>",  "NUMERIC(6,2)",                                            true,  None,              "Actual hours logged"),
            f("due_date",       "Option<String>","DATE",                                                   true,  None,              "Due date"),
            f("position",       "i32",          "INTEGER NOT NULL",                                        false, Some("0"),        "Sort position in column"),
            f("created_at",     "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                      false, Some("NOW()"),    "Creation timestamp"),
            f("updated_at",     "String",       "TIMESTAMPTZ NOT NULL DEFAULT NOW()",                      false, Some("NOW()"),    "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "title_not_empty".into(), condition: "!self.title.trim().is_empty()".into(), message: "Title must not be empty".into() },
            ValidationRule { name: "valid_status".into(), condition: "matches!(self.status.as_str(), \"todo\" | \"in_progress\" | \"in_review\" | \"done\" | \"blocked\")".into(), message: "Invalid task status".into() },
            ValidationRule { name: "valid_priority".into(), condition: "matches!(self.priority.as_str(), \"low\" | \"medium\" | \"high\" | \"critical\")".into(), message: "Invalid priority".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_tasks_project".into(),  columns: vec!["project_id".into()],  unique: false },
            IndexDef { name: "idx_tasks_sprint".into(),   columns: vec!["sprint_id".into()],   unique: false },
            IndexDef { name: "idx_tasks_assignee".into(), columns: vec!["assignee_id".into()], unique: false },
            IndexDef { name: "idx_tasks_status".into(),   columns: vec!["status".into()],      unique: false },
        ],
    }
}

fn pm_comment_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Comment".into(),
        description: "Comment on a task".into(),
        fields: vec![
            f("id",        "i64",   "BIGSERIAL PRIMARY KEY",                                   false, None,           "Primary key"),
            f("task_id",   "i64",   "BIGINT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE",  false, None,           "FK to task"),
            f("author_id", "i64",   "BIGINT NOT NULL REFERENCES users(id)",                    false, None,           "FK to author"),
            f("body",      "String","TEXT NOT NULL",                                            false, None,           "Comment text"),
            f("created_at","String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                       false, Some("NOW()"), "Creation timestamp"),
            f("updated_at","String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                       false, Some("NOW()"), "Update timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "body_not_empty".into(), condition: "!self.body.trim().is_empty()".into(), message: "Comment body must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_comments_task".into(),   columns: vec!["task_id".into()],   unique: false },
            IndexDef { name: "idx_comments_author".into(), columns: vec!["author_id".into()], unique: false },
        ],
    }
}

fn pm_label_entity() -> EntityTemplate {
    EntityTemplate {
        name: "Label".into(),
        description: "Coloured label for tagging tasks".into(),
        fields: vec![
            f("id",         "i64",   "BIGSERIAL PRIMARY KEY",                                     false, None,           "Primary key"),
            f("project_id", "i64",   "BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE", false, None,           "FK to project"),
            f("name",       "String","VARCHAR(100) NOT NULL",                                      false, None,           "Label name"),
            f("color",      "String","VARCHAR(20) NOT NULL",                                       false, Some("'#808080'"),"Hex color"),
            f("created_at", "String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                         false, Some("NOW()"), "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "name_not_empty".into(), condition: "!self.name.trim().is_empty()".into(), message: "Label name must not be empty".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_labels_project".into(), columns: vec!["project_id".into()], unique: false },
        ],
    }
}

fn pm_task_label_entity() -> EntityTemplate {
    EntityTemplate {
        name: "TaskLabel".into(),
        description: "Join table: task ↔ label".into(),
        fields: vec![
            f("task_id",  "i64","BIGINT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE",  false, None, "FK to task"),
            f("label_id", "i64","BIGINT NOT NULL REFERENCES labels(id) ON DELETE CASCADE", false, None, "FK to label"),
        ],
        validations: vec![],
        indices: vec![
            IndexDef { name: "pk_task_labels".into(), columns: vec!["task_id".into(), "label_id".into()], unique: true },
        ],
    }
}

fn pm_team_member_entity() -> EntityTemplate {
    EntityTemplate {
        name: "TeamMember".into(),
        description: "Project team membership with role".into(),
        fields: vec![
            f("id",         "i64",   "BIGSERIAL PRIMARY KEY",                                      false, None,               "Primary key"),
            f("user_id",    "i64",   "BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE",     false, None,               "FK to user"),
            f("project_id", "i64",   "BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE",  false, None,               "FK to project"),
            f("role",       "String","VARCHAR(50) NOT NULL",                                        false, Some("'member'"), "owner|admin|member|viewer"),
            f("created_at", "String","TIMESTAMPTZ NOT NULL DEFAULT NOW()",                          false, Some("NOW()"),    "Creation timestamp"),
        ],
        validations: vec![
            ValidationRule { name: "valid_role".into(), condition: "matches!(self.role.as_str(), \"owner\" | \"admin\" | \"member\" | \"viewer\")".into(), message: "Invalid team role".into() },
        ],
        indices: vec![
            IndexDef { name: "idx_team_members_user".into(),    columns: vec!["user_id".into()],    unique: false },
            IndexDef { name: "idx_team_members_project".into(), columns: vec!["project_id".into()], unique: false },
            IndexDef { name: "uk_team_members".into(),          columns: vec!["user_id".into(), "project_id".into()], unique: true },
        ],
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
