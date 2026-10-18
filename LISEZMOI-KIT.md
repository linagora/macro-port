# Dossier de départ Macro-Port (lot 1)

Ce dossier est prêt à devenir le contenu initial du dépôt `macro-port` sur l'organisation GitHub de LINAGORA. Claude Code y trouve tout ce qu'il faut pour réaliser le lot 1 sans avoir à trancher lui-même les questions de fond.

| Fichier | Contenu |
| --- | --- |
| `CLAUDE.md` | Règles de travail pour Claude Code : périmètre, interdits, commandes, format des demandes de fusion avec résumé en français |
| `docs/SPEC-LOT1.md` | Spécification du lot 1 en cinq jalons (M1 extraction, M2 analyse, M3 règles, M4 rapports et CLI, M5 WebAssembly et mini-application web) |
| `docs/ACCEPTANCE.md` | Critères d'acceptation mesurables (A1 à A9), tous automatisés sauf deux revues manuelles |
| `docs/adr/0001-framing-decisions.md` | Décisions prises le 18 octobre 2026 (nom, dépôt, licence, périmètre, plateformes, langue, relecture) |
| `docs/CORPUS-PLAN.md` | Plan pour compléter le corpus, en particulier les classeurs binaires indispensables au jalon M1 |
| `schema/report.schema.json` | Contrat du rapport JSON, avec un exemple validé dans `schema/examples/` |
| `rules/families.yaml` | Les 13 familles d'usage et leurs signaux pondérés |
| `rules/security.yaml` | Signaux de sécurité, chaînes de signaux et indicateurs de conteneur |
| `rules/complexity.yaml` | Seuils de complexité et traits (macro enregistrée, triviale, possiblement inutilisée) |
| `corpus/` | Corpus v0.1 : 80 échantillons, manifeste, vérité terrain sécurité |
| `tools/fixtures/Build-Fixtures.ps1` | Script à lancer une fois sur un poste Windows avec Excel pour fabriquer les classeurs de test |
| `tools/fixtures/make_malformed.py` | Génère les fichiers corrompus pour les tests de robustesse |
| `tools/rules-prototype/prototype.py` | Prototype Python de calibrage des règles (référence, à ne pas porter en Rust) |
| `PROMPT-CLAUDE-CODE.md` | La première commande à donner à Claude Code |

## Relecture

Le dossier a fait l'objet d'une revue technique indépendante, qui a relevé 15 points, tous corrigés dans cette version. Les principaux :

- **Format MS-OVBA** : vrai mode de détection de la protection d'un projet VBA, type de module, pièges connus du flux `dir`.
- **Fichiers .xls chiffrés** : leur code VBA reste lisible et il faut l'analyser.
- **Mini-application web** : politique de sécurité du contenu (CSP) compatible avec WebAssembly.
- **Rapports HTML et CSV** : protection contre l'injection de code.
- **Anonymisation** : règles champ par champ, avec une clé de hachage.
- **Résultats reproductibles** : règle de départage entre familles et convention de numérotation des lignes.
- **Script de fabrication des classeurs** : prise en compte des modules créés par défaut par Excel.

## Mesures de référence

Le prototype naïf, appliqué aux règles v0.1.0 sur le corpus texte, classe correctement 70 % des fichiers en première famille et 88 % dans les deux premières, et détecte les 17 signaux de sécurité attendus. Les critères d'acceptation exigent mieux de l'implémentation Rust : 75 % et 90 %.

## Avant de lancer Claude Code

1. Créer le dépôt `linagora/macro-port` et y pousser ce dossier.
2. Faire tourner `Build-Fixtures.ps1` sur un poste Windows avec Excel. Protéger ensuite à la main trois projets VBA (environ une heure en tout). Sans ces classeurs, le jalon M1 ne peut pas être validé. Claude Code peut toutefois commencer par M2 et M3 sur le corpus texte en attendant.
3. Relire la vérité terrain du corpus (familles et cibles), au plus tard avant le jalon M3.
