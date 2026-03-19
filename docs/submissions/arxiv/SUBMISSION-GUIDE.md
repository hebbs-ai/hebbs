# arXiv Submission Guide for HEBBS

## Can You Submit as an Individual?

**Yes.** arXiv accepts submissions from individuals. No academic affiliation is required. The only gate is the **endorsement system** for first-time submitters.

---

## Step-by-Step Process

### Step 1: Create an arXiv Account
- Go to https://arxiv.org/user/register
- Use a real name and email
- Set up an ORCID profile if you don't have one (strengthens credibility)

### Step 2: Get Endorsed (First-Time Only)
This is the only potential blocker. For **cs.AI** (our target category):

- **What it is**: An existing arXiv author in cs.AI must vouch that your work is legitimate research
- **Who can endorse**: Anyone who has published papers in the cs.AI endorsement domain on arXiv
- **How to find endorsers**:
  1. Look at papers you cite in your work on arXiv
  2. Click "Which authors of this paper are endorsers?" at the bottom of any abstract page
  3. Contact them with a brief professional email + link to your draft
  4. You only need **one** positive endorsement
- **Alternative**: Some institutions grant automatic endorsement. Check if any co-author has this.
- **Tips**:
  - Don't mass-email endorsers. Pick 2-3 who would find HEBBS relevant.
  - Include a PDF of the paper draft so they can assess quality.
  - Researchers in the agent memory space (authors of A-MEM, AgeMem, etc.) are natural candidates.
  - Hugging Face forums and ResearchGate have threads of people seeking/offering endorsements.

### Step 3: Prepare the Paper
- **Preferred format**: LaTeX (strongly preferred over PDF)
- **Template**: Use the NeurIPS 2026 LaTeX template (works for arXiv too, and we'll reuse it)
- **Files to include**:
  - `main.tex` (the paper)
  - `references.bib` (bibliography)
  - `figures/` (all figures as PDF/PNG)
  - Any style files (neurips_2026.sty, etc.)
- **No page limit** on arXiv (but keep NeurIPS structure: 8 pages + appendix)

### Step 4: Choose Categories
- **Primary**: `cs.AI` (Artificial Intelligence)
- **Cross-list**: `cs.CL` (Computation and Language), `cs.LG` (Machine Learning)
- Cross-listing increases visibility across communities

### Step 5: Submit
- Go to https://arxiv.org → "START NEW SUBMISSION"
- Upload all files (zip of LaTeX source)
- Fill in metadata:
  - Title
  - Authors (name + affiliation/independent)
  - Abstract
  - Categories
  - Comments (e.g., "10 pages, 7 figures")
  - License: select one (typically CC BY 4.0 or arXiv non-exclusive)
- arXiv compiles your LaTeX and shows a preview
- Review and approve

### Step 6: Moderation
- All submissions go through moderation (usually 1-2 business days)
- Moderators check: is it on-topic for the category? Is it scientific in nature?
- Rarely rejected for well-formatted, on-topic CS papers
- If held, you can respond to moderator queries

### Step 7: Publication
- Submissions before 14:00 ET are typically live by 20:00 ET same day
- You get an arXiv ID (e.g., `arXiv:2604.XXXXX`)
- Paper is permanently available and citable
- You can submit revisions (v2, v3...) at any time

---

## What's Required (Checklist)

- [ ] arXiv account created
- [ ] ORCID profile set up
- [ ] Endorsement secured for cs.AI
- [ ] Paper in LaTeX format
- [ ] All figures finalized
- [ ] Bibliography complete
- [ ] Abstract written (250 words max recommended)
- [ ] License chosen
- [ ] Co-author consent (if any co-authors)

---

## Timeline

| Task | Target |
|---|---|
| Create arXiv account + seek endorsement | This week |
| Paper draft (sections 1-4) | By Apr 7 |
| Benchmarks running | Apr 7-21 |
| Full draft complete | By Apr 28 |
| Submit to arXiv | May 1-3 (before NeurIPS deadline) |
| NeurIPS abstract | May 4 |
| NeurIPS full paper | May 6 |

---

## Directory Structure

```
arxiv/
  SUBMISSION-GUIDE.md    # This file
  paper/                 # LaTeX source files
    main.tex
    references.bib
    figures/
    neurips_2026.sty
```

---

## References

- arXiv submission guidelines: https://info.arxiv.org/help/submit/index.html
- Endorsement policy: https://info.arxiv.org/help/endorsement.html
- First-time user guide: https://ieeevis.org/year/2024/info/open-practices/arxiv-first-time-user
- Checklist: https://towardsdatascience.com/a-checklist-for-submitting-your-research-to-arxiv-64f31b4127d2/
