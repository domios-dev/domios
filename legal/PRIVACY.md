# Domios Privacy Policy

> DRAFT pending owner and lawyer review. Version 0.1 (draft). Not in force. Statements are based on the current product (`prod-docs/ACCOUNT.md`, the account backend and installation reports); items marked [OPEN] must be confirmed.

## 1. Controller

Individual Entrepreneur Andrey Kovalev (ინდივიდუალური მეწარმე ანდრეი კოვალევ), Georgian identification number 302218142, registered by the National Agency of Public Registry, Tbilisi, Georgia, is the controller of the personal data described here. Contact for privacy requests: g.andry90@gmail.com.

### Which laws apply

We process personal data under the Law of Georgia on Personal Data Protection (2023), supervised by the Personal Data Protection Service of Georgia. Because we offer Domios to people in the EU/EEA and the United Kingdom, the EU General Data Protection Regulation (GDPR Art. 3(2)(a)) and the UK GDPR also apply to their data. [OPEN: EU representative under GDPR Art. 27 and UK representative under UK GDPR Art. 27. The Art. 27(2) exemption (occasional, low-risk processing, no large-scale special categories) is arguable for accounts and opt-in analytics but regular installation reporting makes processing non-occasional; the cautious default is to appoint one.]

## 2. What stays on your machine

Domios runs locally. Your prompts, messages, terminal output, files, paths, workspace and tab names, memory, sessions and credentials stay on your machines. We do not receive them.

## 3. What we process

| Data | When | Purpose | Legal basis (GDPR Art. 6) | Retention |
|---|---|---|---|---|
| Account: Firebase UID, email address, display name, sign-in provider (Google, Facebook or email) | You create an account or sign in | Provide the account, sign-in and support | Contract, 6(1)(b) | Until you delete the account (see Section 6) [OPEN: backup period] |
| Plan and entitlement: plan, status, period end, cancellation flag, Paddle subscription and customer identifiers | You buy or manage a plan | Grant paid access and sign entitlements | Contract, 6(1)(b); legal obligation for records, 6(1)(c) | Account lifetime; billing records as tax law requires [OPEN: period] |
| Installation reports: random installation ID, machine label, platform, app version, installed plugins with versions and enabled state, counts of workspaces, tabs and memory items, last-seen time | The signed-in app reports periodically | Apply plan limits to machines, show your installations, support | Contract, 6(1)(b) | 30 days after the last report |
| Mesh: machine endpoint and pairing data; traffic relayed between your machines | You enroll machines in Mesh | Connect your machines | Contract, 6(1)(b) | Endpoint until withdrawn or sign-out [OPEN: confirm the relay stores no traffic or metadata logs] |
| Product analytics: event name and catalog properties, random installation ID, event ID, timestamp, and your Firebase UID if signed in | Only if you opt in; off by default | Understand product use | Consent, 6(1)(a) | [OPEN: retention period] |
| Server logs: IP address and request metadata | Any request to our servers | Security, abuse prevention, operation | Legitimate interests, 6(1)(f) | [OPEN: period] |
| Crash reports | [OPEN: no crash-report upload found in the product docs; confirm none is sent] | | | |

Analytics never include prompts, messages, terminal output, paths, workspace or tab names, email addresses, display names, session IDs, peer identities, credentials or model names. The full event catalog is in Settings > Privacy > What we collect. Turn analytics off there or with `domios analytics disable`; this clears queued events and identifiers.

Payment details such as card numbers and billing address are collected by Paddle as Merchant of Record under Paddle's privacy policy. We do not receive card numbers.

## 4. Processors and recipients

- Google (Firebase Authentication, Cloud Firestore, Cloud Functions, Hosting): account, entitlement, installation and analytics storage. [OPEN: region]
- Paddle.com Market Ltd: Merchant of Record for payments, an independent controller for payment data.
- Google and Facebook: only if you choose them to sign in.
- [OPEN: analytics vendor, website host (Vercel), email delivery provider, relay host]

## 5. International transfers

We are established in Georgia, and our providers process data in other countries, including the United States: Google (Firebase), Paddle (United Kingdom and United States) and Vercel (website hosting). For EU/EEA and UK users, transfers to Georgia and onward rely on Standard Contractual Clauses (and the UK Addendum), or on the EU-US Data Privacy Framework where the recipient is certified; Georgian law transfers rely on the same safeguards. [OPEN: Georgia has no EU adequacy decision; confirm per-provider mechanism and data locations.]

## 6. Your rights

You may access, rectify, erase, restrict or object to processing of your data, receive it in a portable form, and withdraw consent at any time without affecting earlier processing. Contact g.andry90@gmail.com. You can complain to the Personal Data Protection Service of Georgia (<https://personaldata.ge>), and, if you are in the EU/EEA or UK, to the supervisory authority where you live or work (in the UK, the ICO).

### Deleting your account

To delete your account, email g.andry90@gmail.com from the address on the account and ask for deletion. We will delete your account and all personal data associated with it, including your entitlement and installation records, and confirm within one month of your request (GDPR Art. 12(3)); we may ask you to verify that the account is yours. Records that tax law requires us to keep, and payment data held by Paddle as an independent controller, are retained only as the law requires. Data on your own machines is not affected.

## 7. Automated decisions

We make no decisions with legal or similarly significant effects based solely on automated processing. Plan limits apply automatically according to your plan.

## 8. Children

Domios is not directed at children under 16. [OPEN: confirm age]

## 9. Changes

We publish each version at https://domios.dev/privacy with a version number and date, and email registered users about material changes before they take effect.
