# Grimoire

**A decentralized, community-powered multimedia platform.**

Grimoire is an ambitious multimedia platform designed to bring video
streaming, music playback, and digital book reading together in one
unified experience. It combines centralized identity management with
distributed, user-contributed media storage and delivery infrastructure.

Rather than depending entirely on a single operator's servers to store
and deliver every file, Grimoire is designed to let participating
content sources contribute storage and bandwidth to the network---while
retaining control over their content, infrastructure, and sharing
permissions.

Grimoire aims to bring together the features people appreciate across
existing media platforms with a scalable, community-powered
architecture.

> **Project status:** Grimoire is in its planning and early development
> phase. Architecture, implementation details, supported integrations,
> and deployment instructions are still being established. This README
> describes the intended goals and behavior, not a claim that every
> feature is currently implemented.

## Table of Contents

-   [Vision](#vision)
-   [Core Features and Goals](#core-features-and-goals)
-   [Unified Multimedia Experience](#unified-multimedia-experience)
-   [Distributed Content Sources and
    CDN](#distributed-content-sources-and-cdn)
-   [Media Acquisition](#media-acquisition)
-   [Content Ownership and
    Permissions](#content-ownership-and-permissions)
-   [Identity and Architecture](#identity-and-architecture)
-   [Security Principles](#security-principles)
-   [Scalability and Resilience](#scalability-and-resilience)
-   [Project Scope](#project-scope)
-   [Planned Architecture
    Considerations](#planned-architecture-considerations)
-   [Development Status](#development-status)
-   [Contributing](#contributing)
-   [Responsible Use](#responsible-use)
-   [License](#license)

## Vision

Grimoire's goal is to build a secure, decentralized, community-powered
multimedia platform that unifies video streaming, music playback, and
digital book reading. It will leverage distributed, user-contributed
storage and CDN infrastructure to achieve scalable content delivery
while maintaining centralized identity management, granular content
ownership, and enforceable authorization policies.

The platform is intended to serve as an alternative to existing media
consumption services. Video, music, and books are all first-class parts
of the project; Grimoire is not intended to be a video-only service with
secondary media features bolted on.

The underlying principle is:

**Contributing storage or bandwidth to Grimoire should not require
giving up ownership or control over who can access a contributor's
media.**

## Core Features and Goals

Grimoire's intended capabilities include:

-   A unified web application for video, music, and books.
-   A searchable media catalog and discovery experience.
-   Community-contributed media storage and delivery.
-   Distributed CDN/source selection for media delivery.
-   Configurable media acquisition through integrations with the \*Arr
    ecosystem.
-   Content-source visibility and sharing controls.
-   Per-user, per-source acquisition permissions.
-   Centralized authentication and account management in the initial
    design.
-   Granular authorization for media access and acquisition.
-   A modular architecture that can evolve as the platform grows.
-   Security, scalability, and resilience as foundational design goals.

These are project goals. Their implementation status may vary as
development progresses.

## Unified Multimedia Experience

Grimoire brings multiple media types into one application.

### Video

The platform is intended to support video content such as anime,
television, movies, and other supported media. Planned playback
considerations include:

-   Streaming and playback controls
-   Quality selection
-   Audio and subtitle track selection
-   Playback progress and history
-   Media metadata and catalog organization

### Music

Music is an equally important part of Grimoire. The platform is intended
to support:

-   Music library browsing and organization
-   Playback
-   Metadata and discovery
-   User-specific preferences and progress where applicable

### Digital Books

Grimoire also aims to provide an integrated digital reading experience.
Potential supported formats include EPUB and CBZ, with reader features
and customization appropriate to each format.

The specific formats and reader capabilities will be determined during
implementation.

## Distributed Content Sources and CDN

Grimoire is designed around **content sources**: participating hosts or
infrastructure nodes that can store media and, where configured, acquire
and distribute it.

A content source may be operated by the Grimoire platform owner or by a
community contributor. Sources can have different capabilities, storage
locations, media categories, acquisition integrations, and sharing
policies.

### Community-powered delivery

The network is intended to grow as contributors offer storage and
bandwidth. When a viewer requests media, Grimoire can select an
authorized and available source that provides suitable delivery
performance.

Source selection may consider:

-   Network and geographic proximity to the viewer
-   Latency and available bandwidth
-   Source health and availability
-   Whether the source has the requested media
-   Content integrity
-   Access permissions and source-owner policies

The best source for a viewer is not necessarily the source closest to
the original contributor. Grimoire should consider the viewer's delivery
conditions and the availability of suitable copies.

### Source owners retain control

Contributing a source does not make all of its media public. Owners
retain control over source visibility, media sharing, acquisition
permissions, and their own infrastructure.

A node possessing a file does not automatically have permission to serve
it to every user.

## Media Acquisition

In Grimoire, a **media request** can mean requesting that content be
acquired and downloaded to a selected content source. It does not merely
mean asking for permission to watch content that already exists.

Grimoire is intended to coordinate with configurable acquisition
infrastructure, including integrations from the \*Arr ecosystem where
suitable. Potential integrations include:

-   **Sonarr** --- series and anime acquisition
-   **Radarr** --- movie acquisition
-   **Lidarr** --- music acquisition
-   **Prowlarr** --- indexer management for compatible applications
-   **Book acquisition integrations** --- to be evaluated based on
    suitability and maintenance

These integrations are examples of the intended ecosystem; exact support
and configuration requirements remain to be implemented and documented.

### Acquisition workflow

A request is intended to follow a flow similar to this:

1.  A user searches for or selects media in Grimoire.
2.  The user chooses a content source they are allowed to view.
3.  Grimoire checks the user's acquisition policy for that source.
4.  The request is automatically accepted, automatically denied, or sent
    to the source owner for manual review.
5.  An accepted request is passed to the source's configured acquisition
    integration.
6.  The acquired media is processed and indexed.
7.  The media becomes accessible according to the applicable sharing and
    viewing permissions.

**Being allowed to request an acquisition does not automatically grant
permission to view the resulting media.** Acquisition authorization and
media viewing authorization are separate.

## Content Ownership and Permissions

Grimoire is designed to provide explicit access control for content
sources and their media.

### Source visibility

Users may request media from content sources they have permission to
view. Sources that are invite-only or otherwise restricted should not be
exposed to unauthorized users.

Visibility is separate from acquisition permission: a user who can view
a source may still be prohibited from requesting downloads to it.

### Per-user acquisition policies

Content source owners can configure acquisition behavior on a **per-user
basis**. The intended policies are:

  -----------------------------------------------------------------------
  Policy                              Behavior
  ----------------------------------- -----------------------------------
  **Auto-accept**                     Requests from the user are accepted
                                      automatically, subject to
                                      applicable checks and limits.

  **Auto-deny**                       Requests from the user are rejected
                                      automatically.

  **Manual approval**                 Each request is presented to the
                                      source owner for approval or
                                      rejection.
  -----------------------------------------------------------------------

The default policy is **auto-deny for everyone**. Source owners may
explicitly increase or change an individual user's permissions at their
discretion.

This default is especially important for private and invite-only
sources: they should not be bombarded with acquisition requests merely
because a user has been granted viewing access.

### Additional safeguards

The platform is expected to account for protections such as:

-   Request rate limiting
-   Duplicate request detection
-   Owner-defined quotas and resource limits
-   Request and decision audit trails
-   Permission changes and revocation
-   Server-side authorization enforcement

Exact limits and workflows remain open design decisions.

## Identity and Architecture

### Centralized identity initially

Grimoire will initially use centralized authentication and identity
management operated by the platform owner. This keeps account security
and administration manageable for the initial public-facing service.

The architecture should preserve the option to explore federated or
decentralized identity in the future. Decentralized authentication is
not an initial requirement and should not be prematurely implemented.

### Control plane and data plane

Grimoire's architecture is intended to distinguish coordination from
media delivery.

**Control plane --- initially centralized:** - Authentication and
identity - User profiles and preferences - Authorization coordination -
Catalog and metadata coordination - Content-source discovery and
configuration - Acquisition request coordination - Administrative
functions

**Data plane --- designed for distribution:** - Media storage - Media
delivery and caching - Source selection - Media transfer - Potential
distributed processing in the future

This separation should allow the platform's coordination services to
remain manageable while the community-powered delivery network grows.

The exact application architecture, programming languages, frameworks,
databases, and service topology have not yet been finalized.

## Security Principles

Grimoire is intended to be publicly available, so security is a
foundational requirement.

The design and implementation should prioritize:

-   Least-privilege access for users, services, and contributors
-   Server-side authorization checks
-   Secure authentication, sessions, and tokens
-   Explicit access policies and fail-closed authorization behavior
-   Protection of private media and origin resources
-   Safe handling of acquisition credentials and secrets
-   Abuse prevention and resource-exhaustion safeguards
-   Auditable security-sensitive actions
-   Treating community-operated content sources as untrusted by default

The platform is expected to use a suitable protected-media delivery
mechanism, potentially involving short-lived, resource-scoped access
credentials and CDN-side validation. The specific mechanism, token
lifetime, and revocation behavior are still design decisions.

Grimoire can enforce access controls within its own services and
delivery network, but it cannot guarantee that an authorized viewer will
not record or redistribute content after receiving it.

## Scalability and Resilience

Grimoire aims to scale through participation rather than requiring all
storage and delivery capacity to come from one operator.

The architecture should account for:

-   Sources joining and leaving the network
-   Partial failures and source unavailability
-   Distributed content delivery
-   Horizontal expansion of services where appropriate
-   Content integrity
-   Media availability and potential replication
-   Resource constraints on community contributors

Decentralization and scalability are related but distinct goals.
Grimoire should pursue both with pragmatic architecture, avoiding
unnecessary complexity and premature service decomposition.

## Project Scope

### In scope

-   Unified web-based multimedia consumption
-   Video, music, and digital books as first-class media
-   User accounts and centralized authentication initially
-   Distributed, community-contributed media storage and delivery
-   Content-source discovery and access policies
-   Configurable media acquisition integrations
-   Per-user, per-source acquisition permission management
-   Secure media access and delivery
-   A modular and scalable platform architecture

### Not an initial requirement

-   Decentralized user authentication
-   Decentralized platform governance
-   A finalized microservice architecture
-   A finalized technology stack
-   A guarantee that every content source is always online
-   Unrestricted public access to contributor media

These may be revisited as the project develops, but they should not be
assumed.

## Planned Architecture Considerations

The following topics will be decided as development and design progress:

-   Frontend framework and UI architecture
-   Backend language and framework
-   Database technology and data model
-   Monolith versus service decomposition
-   Content-source registration, discovery, and trust
-   CDN protocols and source routing
-   Media replication and availability guarantees
-   Media access tokens, revocation, and CDN validation
-   Acquisition adapters and supported integrations
-   Metadata providers and catalog strategy
-   Storage, bandwidth, and acquisition quotas
-   Deployment, observability, backups, and disaster recovery
-   Future identity federation

Technology choices should be driven by the project requirements,
maintainability, security, and operational needs---not by the assumption
that a particular stack has already been selected.

## Development Status

Grimoire is under active planning and early development. The project
objective and key product principles have been established, but the
final architecture and implementation roadmap are still evolving.

As development progresses, this README should be updated to distinguish
implemented features from planned capabilities and to provide accurate
setup, configuration, and deployment instructions.

## Local Development

The initial application foundation lives in two small projects:

- `web/` is a React and TypeScript single-page app built with Vite.
- `backend/` is a Rust HTTP API built with Axum.

The frontend currently provides a responsive media-library starting
point with video, music, and book views. Its sample catalog is presentation
data only. The Rust service currently exposes `GET /api/health`; it does
not yet implement accounts, catalog persistence, media delivery, or
authorization. The frontend displays whether that API is reachable.

Install Node.js (with npm) and Rust (with Cargo), then run the API in one
terminal:

```sh
cd backend
cargo run
```

In another terminal, start the frontend:

```sh
cd web
npm install
npm run dev
```

Open the local URL printed by Vite. During development, Vite forwards
`/api` requests to the API at `http://127.0.0.1:3000`. The API binds to
loopback by default; deployment binding and frontend/API hosting should
be configured explicitly for the target environment.

### Initial stack choice

React was selected for its broad adoption and contributor familiarity;
the [2025 Stack Overflow Developer Survey](https://survey.stackoverflow.co/2025/technology/)
lists React as the most-used web framework in its survey. Vite provides a small development server
and production bundler, while TypeScript adds static checks to the
frontend. Axum provides a Rust-native HTTP routing layer for the API.
This is a starting point, not a commitment to a database, deployment
model, or service topology.

## Contributing

Contributions, ideas, and discussions are welcome as the project takes
shape.

Before proposing major changes, please consider Grimoire's guiding
principles:

1.  Video, music, and books are equally important.
2.  Content-source owners retain control of their infrastructure and
    media.
3.  Acquisition permission defaults to deny.
4.  Viewing, acquisition, and media access are distinct permissions.
5.  Media delivery should be distributed where possible.
6.  Authentication is centralized initially, with future flexibility.
7.  Public-facing security is foundational.
8.  Architecture should scale without needless complexity.

Please consult the repository's `AGENTS.md` for additional project
scope, engineering guidance, and current open design questions.

## Responsible Use

Grimoire is infrastructure for managing, acquiring, and delivering
multimedia. Users and operators are responsible for ensuring that the
media they acquire, host, share, and distribute is handled in accordance
with applicable laws, licenses, and rights-holder permissions.

The platform's access-control features are intended to respect
contributor choices and do not themselves establish rights to any
content.

## License

A project license has not yet been specified. Licensing information will
be added once the project owner selects a license.
