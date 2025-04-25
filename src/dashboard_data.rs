// src/dashboard_data.rs

use actix_web::{error::ErrorInternalServerError, web, Error, HttpResponse};
use chrono::{Datelike, Utc};
use futures::stream::TryStreamExt;
use mongodb::{
    bson::{doc, from_bson, to_bson, Bson, DateTime as BsonDateTime, Document},
    Collection,
};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;

/// Only budget data comes from the frontend
#[derive(Debug, Deserialize)]
pub struct DashboardInput {
    #[serde(rename = "budgetInput")]
    pub budget_input: BudgetInput,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BudgetInput {
    pub total_annual_budget: f64,
    pub monthly_drains: Vec<f64>,
}

/// Helper: get the dashboard_data collection
fn coll(state: &AppState) -> Collection<Document> {
    state
        .mongodb
        .client
        .database(&state.config.database_name)
        .collection("dashboard_data")
}

/// Compute the full dashboard Document given a team_id and budget input.
async fn compute_full_dashboard(
    team_id: &str,
    budget_input: BudgetInput,
    db: &mongodb::Database,
) -> Result<Document, Error> {
    let mut doc = Document::new();

    // 1) Always include teamId & budgetInput
    doc.insert("teamId", team_id);
    doc.insert(
        "budgetInput",
        to_bson(&budget_input).map_err(ErrorInternalServerError)?,
    );

    // 2) Fetch all project IDs for this team
    let project_docs: Vec<Document> = db
        .collection::<Document>("projects")
        .find(doc! { "team_id": team_id })
        .await
        .map_err(ErrorInternalServerError)?
        .try_collect()
        .await
        .map_err(ErrorInternalServerError)?;
    let project_ids: Vec<String> = project_docs
        .iter()
        .filter_map(|p| p.get_str("project_id").ok().map(String::from))
        .collect();

    // 3) Fetch all tickets for those projects
    let tickets: Vec<Document> = if project_ids.is_empty() {
        Vec::new()
    } else {
        db.collection::<Document>("tickets")
            .find(doc! { "project_id": { "$in": project_ids.clone() } })
            .await
            .map_err(ErrorInternalServerError)?
            .try_collect()
            .await
            .map_err(ErrorInternalServerError)?
    };

    // 4) ticketSummary
    let mut open = 0;
    let mut closed = 0;
    let mut total_days = 0.0;
    for t in &tickets {
        let status = t.get_str("status").unwrap_or("").to_lowercase();
        let is_closed = matches!(status.as_str(), "done" | "closed" | "resolved");
        if is_closed {
            closed += 1;
            if let (Ok(created), Ok(due)) =
                (t.get_datetime("created_at"), t.get_datetime("due_date"))
            {
                let secs = (due.to_chrono() - created.to_chrono()).num_seconds();
                if secs > 0 {
                    total_days += secs as f64 / 86_400.0;
                }
            }
        } else {
            open += 1;
        }
    }
    let total_tickets = tickets.len() as i32;
    let avg_resolution = if closed > 0 {
        (total_days / closed as f64 * 10.0).round() / 10.0
    } else {
        0.0
    };
    doc.insert(
        "ticketSummary",
        doc! {
            "totalTickets": total_tickets,
            "openTickets": open,
            "closedTickets": closed,
            "avgResolutionTime": avg_resolution
        },
    );

    // 5) taskMetrics
    let on_track = closed as i64;
    let delayed = (total_tickets as i64 - on_track).max(0);
    doc.insert("taskMetrics", doc! { "onTrack": on_track, "delayed": delayed });

    // 6) Budget chart calculations
    let current_month = Utc::now().month0() as usize;
    let spent: f64 = budget_input
        .monthly_drains
        .iter()
        .take(current_month + 1)
        .copied()
        .sum();
    let planned = budget_input.total_annual_budget;
    let remaining = (planned - spent).max(0.0);
    doc.insert(
        "budget",
        doc! {
            "categories": ["Resources", "Hardware", "Software", "Misc"],
            "planned":   [planned, planned*0.5, planned*0.3, planned*0.2],
            "spent":     [spent, spent*0.5, spent*0.3, spent*0.2],
            "remaining": [remaining, remaining*0.5, remaining*0.3, remaining*0.2],
        },
    );

    // 7) KPI data
    let budget_pct = if planned > 0.0 {
        (spent / planned * 100.0).round()
    } else {
        0.0
    };
    doc.insert(
        "kpiData",
        doc! {
            "tasksCompleted": on_track,
            "tasksTotal": total_tickets as i64,
            "tasksDelta": format!("{:.1}%", (on_track as f64 / (total_tickets as f64).max(1.0) * 100.0) - 100.0),
            "budgetSpent": spent,
            "budgetPercent": budget_pct,
            "teamVelocity": "On Track",
            "teamVelocityNumeric": closed as i64,
            "teamMorale": "N/A",
            "teamMoraleNumeric": 0.0,
            "teamMoraleLabel": "Medium",
        },
    );

    // 8) Priority distribution
    let (mut high, mut medium, mut low) = (0, 0, 0);
    for t in &tickets {
        let s = t.get_str("status").unwrap_or("").to_lowercase();
        if !matches!(s.as_str(), "done" | "closed" | "resolved") {
            match t.get_str("priority").unwrap_or("").to_lowercase().as_str() {
                "high" => high += 1,
                "medium" => medium += 1,
                "low" => low += 1,
                _ => {}
            }
        }
    }
    doc.insert("priority", doc! { "high": high, "medium": medium, "low": low });

    // 9) Completion timeline by sprint
    let mut sprint_counts = std::collections::BTreeMap::new();
    for t in &tickets {
        if let Some(Bson::Int32(s)) = t.get("sprint").cloned() {
            *sprint_counts.entry(s).or_insert(0) += 1;
        }
    }
    let completion: Vec<Document> = sprint_counts
        .into_iter()
        .map(|(s, cnt)| doc! { "sprint": format!("Sprint {}", s), "completed": cnt })
        .collect();
    doc.insert(
        "completion",
        Bson::Array(completion.into_iter().map(Bson::Document).collect()),
    );

    // 10) Risks vs Issues
    let mut risk_high = [0, 0];
    let mut risk_med = [0, 0];
    let mut risk_low = [0, 0];
    for t in &tickets {
        let st = t.get_str("status").unwrap_or("").to_lowercase();
        if !matches!(st.as_str(), "done" | "closed" | "resolved") {
            let is_issue = t.get_str("ticket_type").unwrap_or("") == "Bug";
            let idx = if is_issue { 1 } else { 0 };
            match t.get_str("priority").unwrap_or("").to_lowercase().as_str() {
                "high" => risk_high[idx] += 1,
                "medium" => risk_med[idx] += 1,
                "low" => risk_low[idx] += 1,
                _ => {}
            }
        }
    }
    doc.insert(
        "risks",
        doc! {
            "high":   Bson::Array(risk_high.iter().map(|&x| Bson::Int32(x)).collect()),
            "medium": Bson::Array(risk_med.iter().map(|&x| Bson::Int32(x)).collect()),
            "low":    Bson::Array(risk_low.iter().map(|&x| Bson::Int32(x)).collect()),
        },
    );

    // 11) Stubs for pending items, morale, timeline, AI task list
    doc.insert("pending", doc! { "actionItems": 0, "decisions": 0, "changeRequests": 0 });
    doc.insert("morale", Bson::Array(vec![]));
    doc.insert("timeline", Bson::Array(vec![]));
    doc.insert("aiTaskList", Bson::Array(vec![]));

    // 12) Project stats
    let total_projects = project_docs.len() as i32;
    doc.insert("projectStats", doc! { "activeProjects": total_projects, "completedProjects": 0 });

    // 13) Chat metrics, upcoming events, working hours stubs
    doc.insert("chatMetrics", doc! { "totalMessages": 0, "avgResponseTime": 0 });
    doc.insert("upcomingEvents", Bson::Array(vec![]));
    doc.insert("workingHours", doc! { "averageStart": "09:00", "averageEnd": "17:00" });

    Ok(doc)
}

/// GET /team-data/{team_id}
pub async fn get_dashboard_data(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let team_id = path.into_inner();
    let dashboards = coll(&state);

    // Pull stored budgetInput (or default zeros)
    let input = dashboards
        .find_one(doc! { "teamId": &team_id })
        .await
        .map_err(ErrorInternalServerError)?
        .and_then(|mut existing| {
            existing
                .remove("budgetInput")
                .and_then(|b| from_bson::<BudgetInput>(b).ok())
        })
        .unwrap_or(BudgetInput {
            total_annual_budget: 0.0,
            monthly_drains: vec![0.0; 12],
        });

    // Recompute everything
    let full = compute_full_dashboard(&team_id, input, &state.mongodb.db)
        .await
        .map_err(ErrorInternalServerError)?;
    Ok(HttpResponse::Ok().json(full))
}

/// PUT /team-data/{team_id}
pub async fn upsert_dashboard_data(
    path: web::Path<String>,
    payload: web::Json<DashboardInput>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let team_id = path.into_inner();
    let input = payload.into_inner().budget_input;

    // Store the raw budgetInput
    let mut base_doc = Document::new();
    base_doc.insert("teamId", &team_id);
    base_doc.insert("budgetInput", to_bson(&input).map_err(ErrorInternalServerError)?);

    let dashboards = coll(&state);
    let filter = doc! { "teamId": &team_id };
    let update = dashboards
        .update_one(filter.clone(), doc! { "$set": &base_doc })
        .await
        .map_err(ErrorInternalServerError)?;
    if update.matched_count == 0 {
        dashboards.insert_one(&base_doc).await.map_err(ErrorInternalServerError)?;
    }

    // Return the freshly computed dashboard
    let full = compute_full_dashboard(&team_id, input, &state.mongodb.db)
        .await
        .map_err(ErrorInternalServerError)?;
    Ok(HttpResponse::Ok().json(full))
}
